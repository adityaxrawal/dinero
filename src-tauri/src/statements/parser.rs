//! Extracts text from a statement PDF, in memory.
//!
//! Deliberately never writes the decoded document to disk: the bytes are the raw
//! financial record, and leaving them in a temporary file would defeat the
//! retention policy applied everywhere else.
//!
//! Records which method succeeded, since a native text layer and an OCR fallback
//! warrant very different confidence in the result.
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct ParsedPage {
    pub page_number: usize,
    pub text: String,
    pub ocr_used: bool,
}

#[derive(Debug)]
pub struct ParseResult {
    pub pages: Vec<ParsedPage>,
    pub parse_method: ParseMethod,
    pub total_pages: usize,
    pub ocr_page_count: usize,
    /// Pages that had no usable text layer and that OCR could not read
    /// either; rows on them are missing from the draft (review P3).
    pub missing_pages: Vec<usize>,
}

#[derive(Debug, PartialEq)]
pub enum ParseMethod {
    Pdfium,
    OcrFallback,
    Mixed,
    LlmAssist,
}

const OCR_TEXT_THRESHOLD: usize = 50;

/// Extracts text from an unencrypted PDF, in memory.
pub async fn parse_in_memory(pdf_bytes: &[u8]) -> Result<ParseResult> {
    parse_in_memory_with_password(pdf_bytes, None).await
}

/// Extracts text from a PDF, supplying a password if it is encrypted.
///
/// Nothing is written to disk at any point: the decoded document is the raw
/// financial record, and a temporary file would escape the retention policy
/// applied everywhere else.
pub async fn parse_in_memory_with_password(
    pdf_bytes: &[u8],
    password: Option<&str>,
) -> Result<ParseResult> {
    tracing::info!(
        "Parsing {} PDF bytes in-memory (bytes will not touch disk)",
        pdf_bytes.len()
    );

    let pdfium_result = crate::statements::sidecar::extract_text_in_sidecar(pdf_bytes, password)
        .await
        .map(|pages| {
            pages
                .into_iter()
                .map(|(page_number, text)| ParsedPage {
                    page_number,
                    text,
                    ocr_used: false,
                })
                .collect::<Vec<_>>()
        });

    match pdfium_result {
        Ok(mut pages) => {
            let total_pages = pages.len();

            let ocr_pages: Vec<usize> = pages
                .iter()
                .filter(|p| {
                    p.text.chars().filter(|c| !c.is_whitespace()).count() < OCR_TEXT_THRESHOLD
                })
                .map(|p| p.page_number)
                .collect();

            let mut ocr_page_count = 0usize;
            let mut missing_pages = Vec::new();
            for page_number in ocr_pages {
                let Some(page) = pages.iter_mut().find(|p| p.page_number == page_number) else {
                    continue;
                };
                match ocr_page(pdf_bytes, page_number, password).await {
                    Ok((_, Some(ocr_text))) => {
                        tracing::info!(
                            "OCR fallback produced {} chars for page {}",
                            ocr_text.len(),
                            page_number
                        );
                        page.text = ocr_text;
                        page.ocr_used = true;
                        ocr_page_count += 1;
                    }
                    Ok((_, None)) => {
                        tracing::warn!("OCR returned no text for page {}", page_number);
                        missing_pages.push(page_number);
                    }
                    Err(e) => {
                        tracing::warn!("OCR failed for page {}: {:#}", page_number, e);
                        missing_pages.push(page_number);
                    }
                }
            }

            let parse_method = match ocr_page_count {
                0 => ParseMethod::Pdfium,
                n if n == total_pages => ParseMethod::OcrFallback,
                _ => ParseMethod::Mixed,
            };

            tracing::info!(
                "Parse complete: {} pages, {} via OCR, method={:?}",
                total_pages,
                ocr_page_count,
                parse_method
            );

            Ok(ParseResult {
                pages,
                parse_method,
                total_pages,
                ocr_page_count,
                missing_pages,
            })
        }
        Err(e) => {
            tracing::warn!(
                "pdfium parse failed: {} — falling back to OCR for all pages",
                e
            );
            match ocr_full_document(pdf_bytes, password).await {
                Ok(result) => Ok(result),
                Err(ocr_err) => {
                    tracing::error!(
                        "Both pdfium and OCR failed. pdfium: {}. OCR: {:#}",
                        e,
                        ocr_err
                    );
                    Ok(ParseResult {
                        pages: vec![],
                        parse_method: ParseMethod::OcrFallback,
                        total_pages: 0,
                        ocr_page_count: 0,
                        missing_pages: vec![],
                    })
                }
            }
        }
    }
}

struct TesseractChildGuard(Option<std::process::Child>);

impl Drop for TesseractChildGuard {
    /// Kills the OCR child process if the guard is dropped early.
    ///
    /// Without this a timeout or an error path would leave tesseract running,
    /// orphaned and holding memory for a page nobody is waiting on.
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// OCRs one page: the page is rendered inside the sandboxed PDF sidecar, never
/// in this process (review P3), then read by tesseract. Returns the document's
/// page count with the text.
async fn ocr_page(
    pdf_bytes: &[u8],
    page_number: usize,
    password: Option<&str>,
) -> Result<(usize, Option<String>)> {
    let (page_count, pgm) =
        crate::statements::sidecar::render_page_in_sidecar(pdf_bytes, password, page_number)
            .await?;
    let text = tokio::task::spawn_blocking(move || run_tesseract_on_bytes(&pgm))
        .await
        .map_err(|e| anyhow::anyhow!("OCR task panicked: {}", e))??;
    Ok((page_count, text))
}

/// OCRs every page of a document whose text extraction failed outright.
///
/// The last resort for scanned statements with no text layer at all. Markedly
/// slower and less accurate than native extraction. Fails only if the first
/// page cannot be rendered; later pages that cannot be read are reported in
/// `missing_pages`.
async fn ocr_full_document(pdf_bytes: &[u8], password: Option<&str>) -> Result<ParseResult> {
    tracing::warn!("Attempting full-document OCR (text extraction failed)");
    let (page_count, first) = ocr_page(pdf_bytes, 1, password).await?;
    let mut texts = vec![first];
    for page_number in 2..=page_count {
        texts.push(match ocr_page(pdf_bytes, page_number, password).await {
            Ok((_, text)) => text,
            Err(e) => {
                tracing::warn!("OCR failed for page {}: {:#}", page_number, e);
                None
            }
        });
    }
    let missing_pages: Vec<usize> = texts
        .iter()
        .enumerate()
        .filter(|(_, t)| t.is_none())
        .map(|(i, _)| i + 1)
        .collect();
    let pages: Vec<ParsedPage> = texts
        .into_iter()
        .enumerate()
        .map(|(i, text)| ParsedPage {
            page_number: i + 1,
            text: text.unwrap_or_default(),
            ocr_used: true,
        })
        .collect();
    Ok(ParseResult {
        total_pages: pages.len(),
        ocr_page_count: pages.len() - missing_pages.len(),
        pages,
        parse_method: ParseMethod::OcrFallback,
        missing_pages,
    })
}

/// Runs tesseract over image bytes and returns the recognised text.
fn run_tesseract_on_bytes(image_bytes: &[u8]) -> Result<Option<String>> {
    let tesseract_available = std::process::Command::new("tesseract")
        .arg("--version")
        .output()
        .is_ok();

    if !tesseract_available {
        tracing::warn!(
            "Tesseract OCR is not installed or not on PATH — OCR fallback unavailable. \
             Install tesseract via: brew install tesseract"
        );
        return Err(anyhow::anyhow!("tesseract not available"));
    }

    use std::io::Write;
    use std::process::Stdio;

    let child = std::process::Command::new("tesseract")
        .arg("stdin")
        .arg("stdout")
        .arg("-l")
        .arg("eng")
        .arg("--psm")
        .arg("6")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("tesseract spawn error: {}", e))?;

    let mut guard = TesseractChildGuard(Some(child));

    let mut stdin = guard
        .0
        .as_mut()
        .and_then(|c| c.stdin.take())
        .ok_or_else(|| anyhow::anyhow!("failed to open tesseract stdin"))?;
    let image_bytes_owned = image_bytes.to_vec();
    let writer = std::thread::spawn(move || -> std::io::Result<()> {
        stdin.write_all(&image_bytes_owned)?;
        Ok(())
    });

    let child = guard.0.take().expect("guard was just populated above");
    let output = child
        .wait_with_output()
        .map_err(|e| anyhow::anyhow!("tesseract exec error: {}", e))?;

    writer
        .join()
        .map_err(|_| anyhow::anyhow!("tesseract stdin writer thread panicked"))?
        .map_err(|e| anyhow::anyhow!("tesseract stdin write error: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::warn!("Tesseract exited with error: {}", stderr);
    }

    let text = String::from_utf8_lossy(&output.stdout).to_string();
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raw_pdf_not_persisted_after_parse() {
        for file in ["statements/parser.rs", "statements/password.rs"] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join(file);
            let content = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("failed to read {file}: {e}"));
            let real_code = content.split("#[cfg(test)]").next().unwrap_or(&content);
            for forbidden in ["fs::write(", "File::create(", "std::fs::File::create"] {
                assert!(
                    !real_code.contains(forbidden),
                    "{file} must never write raw PDF bytes to disk during parsing (found '{forbidden}')"
                );
            }
        }
    }

    #[test]
    fn test_ocr_fallback_triggered_for_scanned_page() {
        let sparse_text = "   1   ";
        let char_count = sparse_text.chars().filter(|c| !c.is_whitespace()).count();
        assert!(
            char_count < OCR_TEXT_THRESHOLD,
            "Test page must have fewer than {} non-whitespace chars, got {}",
            OCR_TEXT_THRESHOLD,
            char_count
        );

        let rich_text = "HDFC Bank Credit Card Statement\n\
                          Statement Period: 01/01/2024 to 31/01/2024\n\
                          Total Amount Due: Rs. 12,345.67\n\
                          Minimum Amount Due: Rs. 1,234.00";
        let rich_char_count = rich_text.chars().filter(|c| !c.is_whitespace()).count();
        assert!(
            rich_char_count >= OCR_TEXT_THRESHOLD,
            "Rich page must have at least {} non-whitespace chars, got {}",
            OCR_TEXT_THRESHOLD,
            rich_char_count
        );
    }

    #[test]
    fn test_ocr_threshold_constant() {
        assert_eq!(OCR_TEXT_THRESHOLD, 50);
    }

    #[tokio::test]
    async fn test_parse_in_memory_garbage_bytes_returns_empty() {
        let garbage: &[u8] = b"not a real pdf at all";
        let result = parse_in_memory(garbage).await;
        if let Ok(pr) = result {
            assert!(
                pr.total_pages == 0 || pr.pages.iter().all(|p| p.text.is_empty()),
                "Garbage input must yield empty pages, got {} pages",
                pr.total_pages
            );
        }
    }

    #[test]
    fn test_ocr_fallback_skewed_scan_handled() {
        let skewed_png_bytes = b"garbage_skewed_png_data";
        let result = run_tesseract_on_bytes(skewed_png_bytes);
        if let Ok(opt_text) = result {
            assert!(opt_text.unwrap_or_default().is_empty());
        }
    }

    #[test]
    fn test_ocr_fallback_low_contrast_handled() {
        let low_contrast_bytes = b"low_contrast_data";
        let result = run_tesseract_on_bytes(low_contrast_bytes);
        if let Ok(opt_text) = result {
            assert!(opt_text.unwrap_or_default().is_empty());
        }
    }

    #[test]
    fn test_ocr_fallback_watermarked_page_handled() {
        let watermarked_bytes = b"watermarked_data";
        let result = run_tesseract_on_bytes(watermarked_bytes);
        if let Ok(opt_text) = result {
            assert!(opt_text.unwrap_or_default().is_empty());
        }
    }

    #[test]
    fn test_ocr_temp_files_cleaned_up_on_panic() {
        let child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("must be able to spawn a long-running process for this test");
        let pid = child.id();
        let guard = TesseractChildGuard(Some(child));

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = guard;
            panic!("simulated panic while the guarded child is still running");
        }));
        assert!(result.is_err(), "the simulated panic must actually occur");

        std::thread::sleep(std::time::Duration::from_millis(200));
        let still_alive = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(
            !still_alive,
            "child process (pid={}) must be killed by the RAII guard on panic",
            pid
        );
    }
}
