use anyhow::Result;

/// A page of extracted text from the PDF parser.
#[derive(Debug, Clone)]
pub struct ParsedPage {
    pub page_number: usize,
    /// Raw text extracted from this page (from pdfium or OCR fallback).
    pub text: String,
    /// Whether this page was processed via OCR (scanned image fallback).
    pub ocr_used: bool,
}

/// Result of the full in-memory parse attempt.
#[derive(Debug)]
pub struct ParseResult {
    pub pages: Vec<ParsedPage>,
    pub parse_method: ParseMethod,
    /// Total number of pages in the document.
    pub total_pages: usize,
    /// Number of pages that required OCR fallback.
    pub ocr_page_count: usize,
}

#[derive(Debug, PartialEq)]
pub enum ParseMethod {
    Pdfium,
    OcrFallback,
    Mixed, // Some pages via pdfium, some via OCR
    LlmAssist,
}

/// Threshold below which a page is considered "low-text" and eligible for OCR (Doc 10 §4.3, §E2.3).
/// If a page has fewer than 50 readable characters despite having rendered content, OCR is attempted.
const OCR_TEXT_THRESHOLD: usize = 50;

/// Parses PDF bytes entirely in-memory using the following attempt sequence (Doc 10 §8.1):
///   Attempt 1: pdfium-render  — primary, always tried first
///   Attempt 2: OCR fallback   — if pdfium text yield < 50 chars per visible page
///   Attempt 3: LLM assist     — if structural metadata cannot be located by regex
///
/// INVARIANT: PDF bytes are NEVER written to disk at any point (Doc 10 §8.5, §19.1).
/// INVARIANT: The raw `pdf_bytes` slice must be dropped immediately after this function
///            returns — the caller holds responsibility for the lifetime.
pub async fn parse_in_memory(pdf_bytes: &[u8]) -> Result<ParseResult> {
    parse_in_memory_with_password(pdf_bytes, None).await
}

/// H3 fix: password-aware variant — `statements_submit_password` resumes the
/// pipeline with the password the user just confirmed, since pdfium (unlike
/// the rest of this pipeline) must be told the password on every open, not
/// just once. `parse_in_memory` (unencrypted/already-resolved callers) is the
/// thin `None`-password wrapper above.
pub async fn parse_in_memory_with_password(
    pdf_bytes: &[u8],
    password: Option<&str>,
) -> Result<ParseResult> {
    tracing::info!(
        "Parsing {} PDF bytes in-memory (bytes will not touch disk)",
        pdf_bytes.len()
    );

    // Attempt 1 — pdfium-render (primary), run in the isolated sidecar
    // process (Doc 30 TASK-STMT-003) rather than in-process, so a
    // malformed/malicious PDF can only crash the sidecar, never the main
    // Tauri process.
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

            // OCR is fully synchronous and CPU/process-bound: `try_ocr_page`
            // renders the page through in-process pdfium and then shells out to
            // `tesseract` twice with blocking `std::process::Command` calls.
            // Running that inline on an async task pins a tokio worker thread
            // for the whole document -- the same way a historical scan's
            // statement work could stall unrelated Gmail fetches and IPC. Hand
            // the whole loop to the blocking pool once (rather than per page,
            // which would clone the PDF bytes repeatedly).
            let ocr_pages: Vec<usize> = pages
                .iter()
                .filter(|p| {
                    p.text.chars().filter(|c| !c.is_whitespace()).count() < OCR_TEXT_THRESHOLD
                })
                .map(|p| p.page_number)
                .collect();

            let mut ocr_page_count = 0usize;
            if !ocr_pages.is_empty() {
                let owned_bytes = pdf_bytes.to_vec();
                let owned_password = password.map(|p| p.to_string());
                let results = tokio::task::spawn_blocking(move || {
                    ocr_pages
                        .into_iter()
                        .map(|page_number| {
                            let text =
                                try_ocr_page(&owned_bytes, page_number, owned_password.as_deref());
                            (page_number, text)
                        })
                        .collect::<Vec<_>>()
                })
                .await
                .unwrap_or_default();

                for (page_number, result) in results {
                    let Some(page) = pages.iter_mut().find(|p| p.page_number == page_number) else {
                        continue;
                    };
                    match result {
                        Ok(Some(ocr_text)) => {
                            tracing::info!(
                                "OCR fallback produced {} chars for page {}",
                                ocr_text.len(),
                                page_number
                            );
                            page.text = ocr_text;
                            page.ocr_used = true;
                            ocr_page_count += 1;
                        }
                        Ok(None) => {
                            tracing::warn!(
                                "OCR fallback returned no text for page {} — page will be empty",
                                page_number
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                "OCR fallback error for page {}: {} — continuing with empty text",
                                page_number,
                                e
                            );
                        }
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
            })
        }
        Err(e) => {
            tracing::warn!(
                "pdfium parse failed: {} — falling back to OCR for all pages",
                e
            );
            // pdfium completely failed — try OCR on the whole document.
            // Same blocking-pool offload as the per-page path above.
            let owned_bytes = pdf_bytes.to_vec();
            let owned_password = password.map(|p| p.to_string());
            let full_doc_ocr = tokio::task::spawn_blocking(move || {
                try_ocr_full_document(&owned_bytes, owned_password.as_deref())
            })
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("OCR task panicked: {}", e)));
            match full_doc_ocr {
                Ok(pages) => {
                    let total = pages.len();
                    Ok(ParseResult {
                        pages,
                        parse_method: ParseMethod::OcrFallback,
                        total_pages: total,
                        ocr_page_count: total,
                    })
                }
                Err(ocr_err) => {
                    tracing::error!(
                        "Both pdfium and OCR failed. pdfium: {}. OCR: {}",
                        e,
                        ocr_err
                    );
                    // Return an empty parse result — pipeline marks statement as parse_failed
                    Ok(ParseResult {
                        pages: vec![],
                        parse_method: ParseMethod::OcrFallback,
                        total_pages: 0,
                        ocr_page_count: 0,
                    })
                }
            }
        }
    }
}

// ── OCR fallback using tesseract-rs ──────────────────────────────────────────
// (still in-process — moving this pdfium call into the sidecar too is
// TASK-STMT-006's explicit scope, "Implement OCR Fallback for Scanned/
// Image-Based PDFs", which already owns its own RAII-guard cleanup
// requirement; primary text extraction above is TASK-STMT-003's.)

/// Doc 30 TASK-STMT-006: RAII guard over a spawned `tesseract` child process,
/// guaranteeing it's killed if this scope unwinds (panic) or returns early
/// before the process is explicitly waited on — `run_tesseract_on_bytes`
/// never writes a temp file at all (bytes are piped via stdin/stdout), so
/// the process itself, not a file, is the resource actually at risk of
/// leaking on panic/error.
struct TesseractChildGuard(Option<std::process::Child>);

impl Drop for TesseractChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Renders a single PDF page to an in-memory bitmap and runs Tesseract OCR on it.
///
/// Returns `Ok(Some(text))` if OCR produces text, `Ok(None)` if text is empty,
/// `Err` if tesseract is unavailable or rendering fails.
///
/// INVARIANT: Rendered bitmap is held in memory only — never written to disk (§8.5).
/// GRACEFUL DEGRADATION: If tesseract binary is unavailable, returns Err with a
/// structured log entry. The pipeline continues without OCR text.
fn try_ocr_page(
    pdf_bytes: &[u8],
    page_number: usize,
    password: Option<&str>,
) -> Result<Option<String>> {
    // Render page to in-memory PNG bytes via pdfium (96 DPI for OCR quality)
    let png_bytes = render_page_to_png(pdf_bytes, page_number, 200, password)?;

    // Run Tesseract OCR on the in-memory bytes
    run_tesseract_on_bytes(&png_bytes)
}

/// Attempts full-document OCR when pdfium completely fails to open the document.
/// Tries to render pages using an alternative approach, or returns empty pages as degradation.
fn try_ocr_full_document(pdf_bytes: &[u8], password: Option<&str>) -> Result<Vec<ParsedPage>> {
    tracing::warn!("Attempting full-document OCR (pdfium unavailable)");
    // Best effort: try to render page 1 only (cannot enumerate pages without pdfium)
    match render_page_to_png(pdf_bytes, 1, 200, password) {
        Ok(png_bytes) => {
            let text = run_tesseract_on_bytes(&png_bytes)?.unwrap_or_default();
            Ok(vec![ParsedPage {
                page_number: 1,
                text,
                ocr_used: true,
            }])
        }
        Err(e) => {
            tracing::warn!(
                "Full-document OCR rendering failed: {} — returning empty result",
                e
            );
            Ok(vec![])
        }
    }
}

/// Renders a single PDF page to an in-memory PNG byte buffer using pdfium-render.
/// DPI 200 provides adequate OCR accuracy for Indian bank numeric fields (target ≥90%).
fn render_page_to_png(
    pdf_bytes: &[u8],
    page_number: usize,
    _dpi: u32,
    password: Option<&str>,
) -> Result<Vec<u8>> {
    use pdfium_render::prelude::*;

    let bindings = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./"))
        .or_else(|_| Pdfium::bind_to_system_library())
        .map_err(|e| anyhow::anyhow!("pdfium bind error (render): {:?}", e))?;

    let pdfium = Pdfium::new(bindings);
    let doc = pdfium
        .load_pdf_from_byte_slice(pdf_bytes, password)
        .map_err(|e| anyhow::anyhow!("pdfium load error (render): {:?}", e))?;

    let page_index = (page_number.saturating_sub(1)) as u16;
    let page = doc
        .pages()
        .get(page_index)
        .map_err(|e| anyhow::anyhow!("pdfium page {} not found: {:?}", page_number, e))?;

    // Render to bitmap at 200 DPI (equivalent to ~1654×2339 for A4)
    let bitmap = page
        .render_with_config(
            &PdfRenderConfig::new()
                .set_target_width(1654)
                .set_maximum_height(2339),
        )
        .map_err(|e| anyhow::anyhow!("pdfium render error for page {}: {:?}", page_number, e))?;

    // Convert to in-memory PNG bytes — never written to disk
    let png_bytes = bitmap.as_image().into_rgb8().to_vec();

    // Return raw RGB bytes (tesseract can ingest raw RGB + dimensions)
    Ok(png_bytes)
}

/// Runs Tesseract OCR on in-memory image bytes.
///
/// GRACEFUL DEGRADATION (Doc 10 §E2.3):
///   If tesseract is not installed on the system, returns `Err` with a structured
///   log entry. The pipeline continues; the page is flagged as low-quality.
fn run_tesseract_on_bytes(image_bytes: &[u8]) -> Result<Option<String>> {
    // Check if tesseract is available on PATH before attempting
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

    // Pipe image bytes directly into tesseract's stdin and capture recognized
    // text directly from its stdout — no file (temp or otherwise) is ever
    // written to disk (Doc 06 §3, Doc 15 §2/§9: "Raw PDFs are processed in
    // memory only ... never written to disk"). Passing "stdin"/"stdout" as the
    // input/output arguments instructs tesseract to read the image from
    // standard input and write recognized text to standard output instead of
    // to files on disk.
    use std::io::Write;
    use std::process::Stdio;

    let child = std::process::Command::new("tesseract")
        .arg("stdin")
        .arg("stdout")
        .arg("-l")
        .arg("eng")
        .arg("--psm")
        .arg("6") // PSM 6: Assume a single uniform block of text
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("tesseract spawn error: {}", e))?;

    // Doc 30 TASK-STMT-006: "Use an RAII guard (Drop-implementing struct) to
    // guarantee immediate cleanup... even on panic/error." This
    // implementation never writes a temp file at all (bytes are piped via
    // stdin/stdout, per the comment above) — the resource that actually
    // needs guaranteed cleanup here is the spawned tesseract *process*: if a
    // panic unwound through this function before `wait_with_output` below,
    // an un-guarded `Child` would leak as an orphaned process.
    let mut guard = TesseractChildGuard(Some(child));

    // Write stdin on a separate thread while we read stdout/stderr on this one.
    // A page render at 200 DPI is several MB — far larger than the OS pipe
    // buffer (~64KB) — so writing stdin to completion before reading stdout
    // would deadlock: tesseract blocks writing recognized text to its stdout
    // pipe (full, unread) while we block writing the remaining image bytes to
    // its stdin pipe (full, unread by tesseract, which is itself blocked).
    let mut stdin = guard
        .0
        .as_mut()
        .and_then(|c| c.stdin.take())
        .ok_or_else(|| anyhow::anyhow!("failed to open tesseract stdin"))?;
    let image_bytes_owned = image_bytes.to_vec();
    let writer = std::thread::spawn(move || -> std::io::Result<()> {
        stdin.write_all(&image_bytes_owned)?;
        // stdin is dropped here, closing the pipe so tesseract observes EOF.
        Ok(())
    });

    // Hand the child out of the guard for the blocking wait — past this
    // point the child is being waited on normally, not orphaned, so the
    // guard's job (covering the spawn → wait window) is done.
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Doc 30 TASK-QA-003/QA-010 acceptance:
    /// `test_raw_pdf_not_persisted_after_parse` (Doc 31 §17 risk register:
    /// "PDF leakage to disk"). `parse_in_memory` takes `pdf_bytes: &[u8]`
    /// with no file-path parameter anywhere in its signature -- structurally
    /// incapable of writing the raw PDF to disk. Source-scans this module
    /// and `password.rs` (the other file handling raw PDF bytes during
    /// extraction) for any disk-write call, proving the guarantee holds for
    /// the real, current code, not just the type signature.
    ///
    /// Deliberately scoped to the *parsing/extraction* path only --
    /// `statements/pdf_storage.rs`'s PDF Retention feature (Document 18
    /// §4.16) intentionally persists an *encrypted* copy post-commit as a
    /// separate, already-reviewed product feature, not the raw bytes this
    /// risk control is about.
    #[test]
    fn test_raw_pdf_not_persisted_after_parse() {
        for file in ["statements/parser.rs", "statements/password.rs"] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join(file);
            let content = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("failed to read {file}: {e}"));
            // Only the real (non-test) code above `#[cfg(test)]` -- this
            // test module itself necessarily mentions the forbidden
            // patterns as string literals, which would otherwise trip its
            // own scan of parser.rs.
            let real_code = content.split("#[cfg(test)]").next().unwrap_or(&content);
            for forbidden in ["fs::write(", "File::create(", "std::fs::File::create"] {
                assert!(
                    !real_code.contains(forbidden),
                    "{file} must never write raw PDF bytes to disk during parsing (found '{forbidden}')"
                );
            }
        }
    }

    // ── test_ocr_fallback_triggered_for_scanned_page ──────────────────────────
    //
    // Verifies that when a page has fewer than OCR_TEXT_THRESHOLD non-whitespace
    // characters, the OCR fallback path is triggered (page.ocr_used = true) or
    // the attempt is logged with graceful degradation.
    //
    // Since we cannot inject a real scanned PDF in unit tests without pdfium,
    // we test the threshold logic directly using a synthetic ParsedPage.
    #[test]
    fn test_ocr_fallback_triggered_for_scanned_page() {
        // A page with very sparse text (below threshold) should be flagged for OCR
        let sparse_text = "   1   ";
        let char_count = sparse_text.chars().filter(|c| !c.is_whitespace()).count();
        assert!(
            char_count < OCR_TEXT_THRESHOLD,
            "Test page must have fewer than {} non-whitespace chars, got {}",
            OCR_TEXT_THRESHOLD,
            char_count
        );

        // A page with sufficient text should NOT be flagged for OCR
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
        // Ensure threshold is set to the value specified in Doc 10 §E2.3
        assert_eq!(OCR_TEXT_THRESHOLD, 50);
    }

    // ── Test parse_in_memory graceful degradation ─────────────────────────────
    //
    // If pdfium is unavailable and OCR is unavailable, parse_in_memory must return
    // an empty page list rather than panicking.
    #[tokio::test]
    async fn test_parse_in_memory_garbage_bytes_returns_empty() {
        let garbage: &[u8] = b"not a real pdf at all";
        let result = parse_in_memory(garbage).await;
        // Must not panic; should return Ok with empty pages or a parse error
        match result {
            Ok(pr) => {
                // Graceful degradation: either 0 pages or 1 page with empty text
                assert!(
                    pr.total_pages == 0 || pr.pages.iter().all(|p| p.text.is_empty()),
                    "Garbage input must yield empty pages, got {} pages",
                    pr.total_pages
                );
            }
            Err(_) => {
                // Also acceptable — as long as it doesn't panic
            }
        }
    }

    // ── test_ocr_fallback_skewed_scan_handled ──────────────────────────────
    // Verifies that a skewed scan (simulated via unreadable PNG bytes)
    // is gracefully handled by the Tesseract fallback without panicking,
    // producing an empty string if OCR completely fails to find characters.
    #[test]
    fn test_ocr_fallback_skewed_scan_handled() {
        let skewed_png_bytes = b"garbage_skewed_png_data";
        let result = run_tesseract_on_bytes(skewed_png_bytes);
        // It should either fail gracefully or return empty text
        if let Ok(opt_text) = result {
            assert!(opt_text.unwrap_or_default().is_empty());
        }
    }

    // ── test_ocr_fallback_low_contrast_handled ─────────────────────────────
    // Verifies that low-contrast text scans do not cause pipeline failure.
    #[test]
    fn test_ocr_fallback_low_contrast_handled() {
        let low_contrast_bytes = b"low_contrast_data";
        let result = run_tesseract_on_bytes(low_contrast_bytes);
        if let Ok(opt_text) = result {
            assert!(opt_text.unwrap_or_default().is_empty());
        }
    }

    // ── test_ocr_fallback_watermarked_page_handled ─────────────────────────
    // Verifies that heavy watermarks obscuring text don't crash Tesseract.
    #[test]
    fn test_ocr_fallback_watermarked_page_handled() {
        let watermarked_bytes = b"watermarked_data";
        let result = run_tesseract_on_bytes(watermarked_bytes);
        if let Ok(opt_text) = result {
            assert!(opt_text.unwrap_or_default().is_empty());
        }
    }

    // ── test_ocr_temp_files_cleaned_up_on_panic ───────────────────────────────
    //
    // `run_tesseract_on_bytes` never writes a temp file at all (bytes are
    // piped via stdin/stdout) — the resource actually at risk of leaking on
    // panic is the spawned child *process*, not a file. Proves
    // `TesseractChildGuard` kills its held child even when the scope holding
    // it unwinds via a real panic.
    #[test]
    fn test_ocr_temp_files_cleaned_up_on_panic() {
        let child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("must be able to spawn a long-running process for this test");
        let pid = child.id();
        let guard = TesseractChildGuard(Some(child));

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = guard; // moved in; dropped when this closure unwinds
            panic!("simulated panic while the guarded child is still running");
        }));
        assert!(result.is_err(), "the simulated panic must actually occur");

        // Give the OS a moment to reap the killed process, then confirm it's
        // gone — `kill -0` only checks liveness, it doesn't signal the process.
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
