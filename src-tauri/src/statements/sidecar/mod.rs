//! Doc 30 TASK-STMT-003: parent-side driver for the `pdf_sidecar` binary —
//! spawns it per request, writes the length-prefixed protocol
//! (`src/bin/pdf_sidecar.rs` documents the exact framing), and enforces a
//! timeout, killing the child rather than waiting indefinitely on a hung or
//! malicious PDF. PDF bytes only ever cross a stdin pipe — never a temp file
//! (Doc 15 Core Principle 4/10).

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

#[cfg(test)]
mod tests;

/// Doc 30 TASK-STMT-003: "enforce a 30-second-per-page execution timeout."
/// For the password unlock check (this task's own scope — it doesn't do
/// per-page work), a flat single-page budget applies.
pub const SIDECAR_TIMEOUT_SECS: u64 = 30;

#[derive(Serialize)]
struct SidecarRequest<'a> {
    operation: &'a str,
    password: Option<&'a str>,
}

#[derive(Deserialize)]
struct UnlockCheckResponse {
    success: bool,
    unlocked: Option<bool>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct SidecarPage {
    page_number: usize,
    text: String,
}

#[derive(Deserialize)]
struct ExtractTextResponse {
    success: bool,
    pages: Option<Vec<SidecarPage>>,
    error: Option<String>,
}

fn sidecar_binary_path() -> PathBuf {
    // Sibling of the running executable — matches how `cargo build`/`cargo
    // test` places `pdf_sidecar` next to the main binary in `target/debug`,
    // and how a Tauri-bundled app places sidecar binaries alongside the main
    // executable. Bundling/signing the sidecar into the release `.app` is
    // TASK-DESK-009's (Configure Tauri Build Pipeline) explicit scope, not
    // reached yet — this resolution already works unmodified once it is.
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("pdf_sidecar")))
        .unwrap_or_else(|| PathBuf::from("pdf_sidecar"))
}

/// Doc 30 TASK-STMT-003: runs the pdfium unlock-check in the isolated sidecar.
pub async fn unlock_check_in_sidecar(pdf_bytes: &[u8], password: &str) -> Result<bool> {
    let meta = serde_json::to_vec(&SidecarRequest {
        operation: "unlock_check",
        password: Some(password),
    })?;
    let output = run_with_timeout(
        sidecar_binary_path(),
        &[],
        &meta,
        pdf_bytes,
        Duration::from_secs(SIDECAR_TIMEOUT_SECS),
    )
    .await?;

    let resp: UnlockCheckResponse = serde_json::from_slice(&output)
        .map_err(|e| anyhow!("malformed sidecar response: {} (raw: {:?})", e, output))?;
    if resp.success {
        Ok(resp.unlocked.unwrap_or(false))
    } else {
        Err(anyhow!(
            "sidecar unlock_check error: {}",
            resp.error.unwrap_or_else(|| "unknown".to_string())
        ))
    }
}

/// Doc 30 TASK-STMT-003 (infrastructure shared with STMT-004/005's page-text
/// consumers): runs pdfium's page-text extraction in the isolated sidecar.
/// Timeout is `30s * page count is unknown up front`, so this uses a
/// generous fixed ceiling instead — refined once a later task can supply a
/// real page-count estimate ahead of the call.
pub async fn extract_text_in_sidecar(
    pdf_bytes: &[u8],
    password: Option<&str>,
) -> Result<Vec<(usize, String)>> {
    let meta = serde_json::to_vec(&SidecarRequest {
        operation: "extract_text",
        password,
    })?;
    let output = run_with_timeout(
        sidecar_binary_path(),
        &[],
        &meta,
        pdf_bytes,
        Duration::from_secs(SIDECAR_TIMEOUT_SECS),
    )
    .await?;

    let resp: ExtractTextResponse = serde_json::from_slice(&output)
        .map_err(|e| anyhow!("malformed sidecar response: {} (raw: {:?})", e, output))?;
    if resp.success {
        Ok(resp
            .pages
            .unwrap_or_default()
            .into_iter()
            .map(|p| (p.page_number, p.text))
            .collect())
    } else {
        Err(anyhow!(
            "sidecar extract_text error: {}",
            resp.error.unwrap_or_else(|| "unknown".to_string())
        ))
    }
}

/// Generic process-isolation primitive: spawn `binary args...`, write a JSON
/// metadata line + a 4-byte-length-prefixed payload to its stdin, read
/// whatever it writes to stdout until EOF, killing the child if it doesn't
/// finish within `timeout`. Kept separate from the pdfium-specific functions
/// above so the isolation/timeout mechanism itself — the actual invariant
/// TASK-STMT-003 cares about — is directly testable without a real PDF or
/// the real `pdf_sidecar` binary.
async fn run_with_timeout(
    binary: PathBuf,
    args: &[&str],
    meta_json: &[u8],
    payload: &[u8],
    timeout: Duration,
) -> Result<Vec<u8>> {
    // ROOT CAUSE FIX: the sidecar looks for libpdfium.dylib via
    // `Pdfium::pdfium_platform_library_name_at_path("./")`, which resolves
    // relative to the *child process's* CWD — not relative to the sidecar
    // binary. Without `.current_dir()` the child inherits the Tauri app's
    // CWD (e.g. the user's home or app bundle root), where libpdfium.dylib
    // does not exist. Setting CWD to the binary's parent directory ensures
    // `./libpdfium.dylib` resolves to the copy placed alongside the binary
    // by the build system.
    let binary_dir = binary
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let mut child = Command::new(&binary)
        .args(args)
        .current_dir(&binary_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| anyhow!("failed to spawn sidecar '{}': {}", binary.display(), e))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("sidecar child has no stdin"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("sidecar child has no stdout"))?;

    let meta_json = meta_json.to_vec();
    let payload = payload.to_vec();
    let round_trip = async move {
        stdin.write_all(&meta_json).await?;
        stdin.write_all(b"\n").await?;
        stdin
            .write_all(&(payload.len() as u32).to_be_bytes())
            .await?;
        stdin.write_all(&payload).await?;
        stdin.shutdown().await?;
        drop(stdin);

        let mut buf = Vec::new();
        stdout.read_to_end(&mut buf).await?;
        Ok::<Vec<u8>, std::io::Error>(buf)
    };

    match tokio::time::timeout(timeout, round_trip).await {
        Ok(Ok(output)) => {
            let _ = child.wait().await;
            Ok(output)
        }
        Ok(Err(e)) => {
            let _ = child.kill().await;
            Err(anyhow!("sidecar I/O error: {}", e))
        }
        Err(_) => {
            // Doc 30 TASK-STMT-003: the whole point — a hung or malicious
            // sidecar is killed, never left to block (or, if it were
            // in-process, crash) the caller indefinitely.
            let _ = child.kill().await;
            Err(anyhow!(
                "sidecar_timeout: exceeded {:?} — child process killed",
                timeout
            ))
        }
    }
}
