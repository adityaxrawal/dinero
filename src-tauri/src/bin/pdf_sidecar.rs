//! Doc 30 TASK-STMT-003: an isolated OS-process sidecar for `pdfium-render`
//! calls. A malformed or malicious PDF can crash or exhaust memory in
//! pdfium's C++ core — running it here, in a short-lived child process
//! spawned per request, means that failure mode takes down this process
//! only, never the main Tauri process (avoiding a macOS Jetsam OOM kill of
//! the whole app). Standalone binary — deliberately has no dependency on the
//! main `dinero_app_lib` crate (DB, Tauri, Keychain); it only ever sees raw
//! PDF bytes over stdin and writes a JSON result to stdout, never touching
//! disk (Doc 15 Core Principle 4/10).
//!
//! ## Protocol (stdin, written by the parent process)
//! 1. One newline-terminated JSON line: `{"operation": "unlock_check" | "extract_text", "password": "..."}`
//! 2. A 4-byte big-endian `u32` length prefix for the PDF payload.
//! 3. The raw PDF bytes (exactly that many bytes).
//!
//! ## Protocol (stdout, written by this process)
//! One newline-terminated JSON line:
//! - `unlock_check`: `{"success": true, "unlocked": bool}` or `{"success": false, "error": "..."}`
//! - `extract_text`: `{"success": true, "pages": [{"page_number": 1, "text": "..."}]}` or `{"success": false, "error": "..."}`

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

#[derive(Deserialize)]
struct Request {
    operation: String,
    #[serde(default)]
    password: Option<String>,
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

fn main() {
    if let Err(e) = run() {
        // Last-resort error path: still emit a well-formed JSON response so
        // the parent's parser never has to guess at a bare stderr message.
        let _ = write_line(&serde_json::json!({ "success": false, "error": e.to_string() }));
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    // One BufReader for the whole request — reading the line via a
    // throwaway BufReader and then switching back to unbuffered `Stdin`
    // reads would silently drop any bytes it had already buffered ahead
    // past the newline, corrupting the length-prefixed payload that follows.
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
        other => {
            write_line(&serde_json::json!({
                "success": false,
                "error": format!("unknown operation: {}", other)
            }))
        }
    }
}

fn unlock_check(pdf_bytes: &[u8], password: &str) -> UnlockCheckResponse {
    use pdfium_render::prelude::*;

    let bindings = match Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./"))
        .or_else(|_| Pdfium::bind_to_system_library())
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
    let pw = if password.is_empty() { None } else { Some(password) };
    let unlocked = pdfium.load_pdf_from_byte_slice(pdf_bytes, pw).is_ok();
    UnlockCheckResponse {
        success: true,
        unlocked: Some(unlocked),
        error: None,
    }
}

fn extract_text(pdf_bytes: &[u8], password: Option<&str>) -> ExtractTextResponse {
    use pdfium_render::prelude::*;

    let bindings = match Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./"))
        .or_else(|_| Pdfium::bind_to_system_library())
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
        let text = page.text().map(|t| t.all()).unwrap_or_default();
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

fn write_line<T: Serialize>(value: &T) -> anyhow::Result<()> {
    let json = serde_json::to_string(value)?;
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    lock.write_all(json.as_bytes())?;
    lock.write_all(b"\n")?;
    lock.flush()?;
    Ok(())
}

/// Small helper trait so `main`'s one-line-read doesn't need a full
/// `BufRead` import juggling act at the call site.
trait ReadLineInto {
    fn read_line_into(&mut self, buf: &mut String) -> std::io::Result<usize>;
}

impl<R: std::io::BufRead> ReadLineInto for R {
    fn read_line_into(&mut self, buf: &mut String) -> std::io::Result<usize> {
        self.read_line(buf)
    }
}
