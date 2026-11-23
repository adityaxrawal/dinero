//! Sandboxed PDF worker process.
//!
//! Runs the unlock, text-extraction, page-rendering (for OCR) and decryption
//! operations out-of-process.
//! Separation is the whole point: PDF parsing is a well-known source of
//! memory-safety bugs and the input is an untrusted file, so a malformed or
//! hostile document crashes this process instead of the application.
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

#[path = "../statements/layout.rs"]
mod layout;

#[derive(Deserialize)]
struct Request {
    operation: String,
    #[serde(default)]
    password: Option<String>,
    /// 1-based page to render (`render_page`).
    #[serde(default)]
    page_number: Option<usize>,
}

#[derive(Serialize)]
struct UnlockCheckResponse {
    success: bool,
    unlocked: Option<bool>,
    error: Option<String>,
}

#[derive(Serialize)]
struct PageOut {
    page_number: usize,
    text: String,
}

#[derive(Serialize)]
struct ExtractTextResponse {
    success: bool,
    pages: Option<Vec<PageOut>>,
    error: Option<String>,
}

#[derive(Serialize)]
struct RenderPageResponse {
    success: bool,
    page_count: Option<usize>,
    /// The page as a grayscale binary PGM, which tesseract reads from stdin.
    pgm_base64: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
struct DecryptResponse {
    success: bool,
    pdf_base64: Option<String>,
    error: Option<String>,
}

/// Sidecar entry point.
fn main() {
    if let Err(e) = run() {
        let _ = write_line(&serde_json::json!({ "success": false, "error": e.to_string() }));
        std::process::exit(1);
    }
}

/// Reads newline-delimited JSON requests and answers each in turn.
///
/// A line protocol over stdio keeps the boundary trivial, which matters because
/// this process exists to be crashed by hostile input without taking the app
/// with it.
fn run() -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let mut reader = std::io::BufReader::new(stdin.lock());

    let mut meta_line = String::new();
    reader.read_line_into(&mut meta_line)?;
    let request: Request = serde_json::from_str(meta_line.trim())?;

    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;

    let mut pdf_bytes = vec![0u8; len];
    reader.read_exact(&mut pdf_bytes)?;

    match request.operation.as_str() {
        "unlock_check" => {
            let password = request.password.unwrap_or_default();
            let resp = unlock_check(&pdf_bytes, &password);
            write_line(&resp)
        }
        "extract_text" => {
            let resp = extract_text(&pdf_bytes, request.password.as_deref());
            write_line(&resp)
        }
        "decrypt" => {
            let resp = decrypt(&pdf_bytes, request.password.as_deref());
            write_line(&resp)
        }
        "render_page" => {
            let resp = render_page(
                &pdf_bytes,
                request.password.as_deref(),
                request.page_number.unwrap_or(1),
            );
            write_line(&resp)
        }
        other => write_line(&serde_json::json!({
            "success": false,
            "error": format!("unknown operation: {}", other)
        })),
    }
}

/// Reports whether a password unlocks a PDF.
fn unlock_check(pdf_bytes: &[u8], password: &str) -> UnlockCheckResponse {
    use pdfium_render::prelude::*;

    let bindings = match bind_pdfium()
    {
        Ok(b) => b,
        Err(e) => {
            return UnlockCheckResponse {
                success: false,
                unlocked: None,
                error: Some(format!("pdfium bind error: {:?}", e)),
            }
        }
    };
    let pdfium = Pdfium::new(bindings);
    let pw = if password.is_empty() {
        None
    } else {
        Some(password)
    };
    let unlocked = pdfium.load_pdf_from_byte_slice(pdf_bytes, pw).is_ok();
    UnlockCheckResponse {
        success: true,
        unlocked: Some(unlocked),
        error: None,
    }
}

/// Extracts text from a PDF, optionally with a password.
fn extract_text(pdf_bytes: &[u8], password: Option<&str>) -> ExtractTextResponse {
    use pdfium_render::prelude::*;

    let bindings = match bind_pdfium()
    {
        Ok(b) => b,
        Err(e) => {
            return ExtractTextResponse {
                success: false,
                pages: None,
                error: Some(format!("pdfium bind error: {:?}", e)),
            }
        }
    };
    let pdfium = Pdfium::new(bindings);
    let doc = match pdfium.load_pdf_from_byte_slice(pdf_bytes, password) {
        Ok(d) => d,
        Err(e) => {
            return ExtractTextResponse {
                success: false,
                pages: None,
                error: Some(format!("pdfium load error: {:?}", e)),
            }
        }
    };

    let mut pages = Vec::new();
    for (idx, page) in doc.pages().iter().enumerate() {
        let text = page
            .text()
            .map(|t| {
                let page_height = page.height().value;
                let chars: Vec<layout::PositionedChar> = t
                    .chars()
                    .iter()
                    .filter_map(|ch| {
                        let bounds = ch.loose_bounds().ok()?;
                        Some(layout::PositionedChar {
                            text: ch.unicode_string()?,
                            x0: bounds.left().value,
                            x1: bounds.right().value,
                            y0: page_height - bounds.top().value,
                            y1: page_height - bounds.bottom().value,
                        })
                    })
                    .collect();
                let rebuilt = layout::reconstruct_page(&chars);
                if rebuilt.trim().is_empty() {
                    t.all()
                } else {
                    rebuilt
                }
            })
            .unwrap_or_default();
        pages.push(PageOut {
            page_number: idx + 1,
            text,
        });
    }

    if pages.is_empty() {
        return ExtractTextResponse {
            success: false,
            pages: None,
            error: Some("pdfium returned 0 pages".to_string()),
        };
    }

    ExtractTextResponse {
        success: true,
        pages: Some(pages),
        error: None,
    }
}

/// Renders one page to a grayscale PGM for OCR (review P3: rendering used to
/// happen in the app process, outside this sandbox).
fn render_page(pdf_bytes: &[u8], password: Option<&str>, page_number: usize) -> RenderPageResponse {
    use pdfium_render::prelude::*;
    let fail = |error: String| RenderPageResponse {
        success: false,
        page_count: None,
        pgm_base64: None,
        error: Some(error),
    };

    let bindings = match bind_pdfium() {
        Ok(b) => b,
        Err(e) => return fail(format!("pdfium bind error: {:?}", e)),
    };
    let pdfium = Pdfium::new(bindings);
    let doc = match pdfium.load_pdf_from_byte_slice(pdf_bytes, password) {
        Ok(d) => d,
        Err(e) => return fail(format!("pdfium load error: {:?}", e)),
    };
    let page_count = doc.pages().len() as usize;
    if page_number == 0 || page_number > page_count {
        return fail(format!("page {} out of range (1..={})", page_number, page_count));
    }
    let page = match doc.pages().get((page_number - 1) as u16) {
        Ok(p) => p,
        Err(e) => return fail(format!("pdfium page {} error: {:?}", page_number, e)),
    };
    let bitmap = match page.render_with_config(
        &PdfRenderConfig::new()
            .set_target_width(1654)
            .set_maximum_height(2339),
    ) {
        Ok(b) => b,
        Err(e) => return fail(format!("pdfium render error for page {}: {:?}", page_number, e)),
    };
    let gray = bitmap.as_image().into_luma8();
    let mut pgm = format!("P5\n{} {}\n255\n", gray.width(), gray.height()).into_bytes();
    pgm.extend_from_slice(gray.as_raw());

    use base64::Engine;
    RenderPageResponse {
        success: true,
        page_count: Some(page_count),
        pgm_base64: Some(base64::engine::general_purpose::STANDARD.encode(&pgm)),
        error: None,
    }
}

/// Decrypts a PDF and returns its bytes.
fn decrypt(pdf_bytes: &[u8], password: Option<&str>) -> DecryptResponse {
    use pdfium_render::prelude::*;

    let bindings = match bind_pdfium()
    {
        Ok(b) => b,
        Err(e) => {
            return DecryptResponse {
                success: false,
                pdf_base64: None,
                error: Some(format!("pdfium bind error: {:?}", e)),
            }
        }
    };
    let pdfium = Pdfium::new(bindings);
    let doc = match pdfium.load_pdf_from_byte_slice(pdf_bytes, password) {
        Ok(d) => d,
        Err(e) => {
            return DecryptResponse {
                success: false,
                pdf_base64: None,
                error: Some(format!("pdfium load error: {:?}", e)),
            }
        }
    };

    match doc.save_to_bytes() {
        Ok(bytes) => {
            use base64::Engine;
            DecryptResponse {
                success: true,
                pdf_base64: Some(base64::engine::general_purpose::STANDARD.encode(&bytes)),
                error: None,
            }
        }
        Err(e) => DecryptResponse {
            success: false,
            pdf_base64: None,
            error: Some(format!("pdfium save error: {:?}", e)),
        },
    }
}

/// Writes one JSON response line.
fn write_line<T: Serialize>(value: &T) -> anyhow::Result<()> {
    let json = serde_json::to_string(value)?;
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    lock.write_all(json.as_bytes())?;
    lock.write_all(b"\n")?;
    lock.flush()?;
    Ok(())
}

trait ReadLineInto {
    /// Reads a request line from stdin.
    fn read_line_into(&mut self, buf: &mut String) -> std::io::Result<usize>;
}

impl<R: std::io::BufRead> ReadLineInto for R {
    /// Reads a request line from a generic reader, used in tests.
    fn read_line_into(&mut self, buf: &mut String) -> std::io::Result<usize> {
        self.read_line(buf)
    }
}

/// Loads libpdfium from the first place it exists: next to the sidecar (and
/// the working directory, which is set to the sidecar's folder), its `build/`
/// subfolder (dev builds, where build.rs leaves it in target/<profile>/build),
/// or the app bundle's Resources folder; then the system library.
fn bind_pdfium() -> Result<Box<dyn pdfium_render::prelude::PdfiumLibraryBindings>, pdfium_render::prelude::PdfiumError> {
    use pdfium_render::prelude::*;
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let mut dirs = vec![std::path::PathBuf::from("./")];
    if let Some(d) = exe_dir {
        dirs.push(d.join("build"));
        dirs.push(d.join("../Resources"));
        dirs.push(d);
    }
    for dir in dirs {
        let lib = Pdfium::pdfium_platform_library_name_at_path(&dir);
        if lib.exists() {
            if let Ok(b) = Pdfium::bind_to_library(lib) {
                return Ok(b);
            }
        }
    }
    Pdfium::bind_to_system_library()
}
