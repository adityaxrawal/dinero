//! Runs PDF operations in a sandboxed sidecar process.
//!
//! Unlocking, text extraction and decryption all happen out-of-process. PDF
//! parsing is a historically rich source of memory-safety vulnerabilities and the
//! input is an untrusted file, so a malformed or hostile document can crash the
//! sidecar without compromising the application.
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

#[cfg(test)]
mod tests;

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

#[derive(Deserialize)]
struct DecryptResponse {
    success: bool,
    pdf_base64: Option<String>,
    error: Option<String>,
}

fn sidecar_binary_path() -> PathBuf {
    let exe = std::env::current_exe().ok();
    let sibling = exe
        .as_ref()
        .and_then(|p| p.parent())
        .map(|dir| dir.join("pdf_sidecar"));
    if let Some(ref path) = sibling {
        if path.exists() {
            return path.clone();
        }
    }
    exe.as_ref()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .map(|dir| dir.join("pdf_sidecar"))
        .filter(|p| p.exists())
        .or(sibling)
        .unwrap_or_else(|| PathBuf::from("pdf_sidecar"))
}

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

pub async fn decrypt_pdf_in_sidecar(pdf_bytes: &[u8], password: &str) -> Result<Vec<u8>> {
    let meta = serde_json::to_vec(&SidecarRequest {
        operation: "decrypt",
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

    let resp: DecryptResponse = serde_json::from_slice(&output)
        .map_err(|e| anyhow!("malformed sidecar response: {} (raw: {:?})", e, output))?;
    if resp.success {
        use base64::Engine;
        let b64 = resp
            .pdf_base64
            .ok_or_else(|| anyhow!("sidecar decrypt: success but no pdf_base64"))?;
        base64::engine::general_purpose::STANDARD
            .decode(&b64)
            .map_err(|e| anyhow!("sidecar decrypt: malformed base64: {}", e))
    } else {
        Err(anyhow!(
            "sidecar decrypt error: {}",
            resp.error.unwrap_or_else(|| "unknown".to_string())
        ))
    }
}

async fn run_with_timeout(
    binary: PathBuf,
    args: &[&str],
    meta_json: &[u8],
    payload: &[u8],
    timeout: Duration,
) -> Result<Vec<u8>> {
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
            let _ = child.kill().await;
            Err(anyhow!(
                "sidecar_timeout: exceeded {:?} — child process killed",
                timeout
            ))
        }
    }
}
