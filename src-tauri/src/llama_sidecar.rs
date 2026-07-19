//! Sidecar process manager for `llama-server` (llama.cpp) — replaces the
//! prior in-process Candle inference path in `extraction/llm.rs`, which had
//! no working loader for any of the 5-tier catalog's actual GGUF
//! architectures: Gemma 4's own `"gemma4"` architecture tag (`candle`'s
//! newest quantized Gemma loader only covers `"gemma3"`), and Qwen3.6's
//! Gated-DeltaNet-hybrid MoE design (not a standard transformer `candle` has
//! any loader for at all). llama.cpp added real Gemma 4 support at launch
//! (April 2026) and tracks new architectures far faster than `candle`'s
//! bindings — this shells out to its own release binary instead of waiting
//! on upstream `candle-transformers` support that doesn't exist yet.
//!
//! Unlike `statements/sidecar` (spawn fresh, write stdin, read stdout,
//! kill — per request), this process is long-lived: the dominant cost of
//! LLM inference is loading multi-GB GGUF weights, not the completion
//! itself, so re-spawning per email would blow past Doc 30's 10-second
//! Layer 6 timeout on model load time alone, every single time. Started
//! lazily on first use, kept warm across the whole app session, restarted
//! only if the user changes the active model. A crash or OOM inside
//! `llama-server` now stays isolated to that separate OS process — the same
//! isolation rationale `bin/pdf_sidecar.rs` already established for
//! pdfium, just applied here to inference instead of PDF parsing.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

const LLAMA_CPP_RELEASE_TAG: &str = "b10068";
const LLAMA_SERVER_PORT: u16 = 58121;
const CONTEXT_SIZE: &str = "2048";

/// How long a fresh `llama-server` process gets to finish loading a
/// multi-GB model before it's considered a failed startup and killed. Only
/// applies to the one-time (per model, per app session) cold start, run in
/// a background task — never blocks any single email's own Layer 6 call,
/// which stays bounded by `LlmEngine::INFERENCE_TIMEOUT` regardless.
const SERVER_STARTUP_TIMEOUT: Duration = Duration::from_secs(120);
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Don't hammer a server that just failed to start (e.g. no network for the
/// binary download) on every single email in a large scan.
const FAILURE_COOLDOWN: Duration = Duration::from_secs(60);

/// (release asset filename, its real SHA-256) — the SHA-256 is computed
/// directly from the downloaded release asset (`shasum -a 256`), the same
/// verify-don't-fabricate standard as `llm_manager::get_available_models`'s
/// GGUF/tokenizer hashes. Re-verify if `LLAMA_CPP_RELEASE_TAG` ever changes.
#[cfg(target_arch = "aarch64")]
fn release_asset() -> (&'static str, &'static str) {
    (
        "llama-b10068-bin-macos-arm64.tar.gz",
        "13aa2d40c76ad1dcb8ebeec5f0d2814bf3b2f84a66935c7d4dc6f7cca8e38d68",
    )
}
#[cfg(not(target_arch = "aarch64"))]
fn release_asset() -> (&'static str, &'static str) {
    (
        "llama-b10068-bin-macos-x64.tar.gz",
        "73a63a0fdcfd8d0625fe20aa8f2af62e3d6437c6380b46129ca1a9abacbde0d5",
    )
}

fn base_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("llama_cpp")
}

fn extracted_dir(app_dir: &Path) -> PathBuf {
    base_dir(app_dir).join(format!("llama-{LLAMA_CPP_RELEASE_TAG}"))
}

fn server_binary_path(app_dir: &Path) -> PathBuf {
    extracted_dir(app_dir).join("llama-server")
}

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

enum ServerState {
    NotStarted,
    Starting,
    Ready { port: u16 },
    Failed { reason: String, at: Instant },
}

struct SidecarState {
    state: ServerState,
    child: Option<Child>,
    model_id: Option<String>,
}

fn state() -> &'static Mutex<SidecarState> {
    static STATE: OnceLock<Mutex<SidecarState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(SidecarState {
            state: ServerState::NotStarted,
            child: None,
            model_id: None,
        })
    })
}

/// Downloads (if not already present) and extracts the llama.cpp release
/// tarball. `LC_RPATH` in the shipped `llama-server` binary is
/// `@loader_path` (verified via `otool -l`), so its `.dylib` dependencies
/// resolve relative to wherever the binary itself sits — extracting the
/// whole tarball intact and always invoking the binary by its full path is
/// sufficient, no `DYLD_LIBRARY_PATH`/cwd tricks needed.
async fn ensure_binary(app_dir: &Path) -> Result<PathBuf> {
    let binary_path = server_binary_path(app_dir);
    if binary_path.exists() {
        return Ok(binary_path);
    }

    let base = base_dir(app_dir);
    tokio::fs::create_dir_all(&base)
        .await
        .context("failed to create llama_cpp directory")?;

    let (asset_name, expected_sha256) = release_asset();
    let url = format!(
        "https://github.com/ggml-org/llama.cpp/releases/download/{LLAMA_CPP_RELEASE_TAG}/{asset_name}"
    );
    let tarball_path = base.join(asset_name);
    crate::llm_manager::download_file_with_hash(&url, &tarball_path, expected_sha256, None)
        .await
        .context("failed to download llama.cpp release")?;

    let status = Command::new("tar")
        .arg("-xzf")
        .arg(&tarball_path)
        .arg("-C")
        .arg(&base)
        .status()
        .await
        .context("failed to run tar to extract llama.cpp release")?;
    if !status.success() {
        anyhow::bail!("tar extraction of llama.cpp release exited with {status}");
    }
    let _ = tokio::fs::remove_file(&tarball_path).await;

    // Best-effort Gatekeeper cleanup. Verified this specific release
    // binary already ships ad-hoc linker-signed (`codesign -dv` shows
    // flags=0x20002(adhoc,linker-signed)) and a programmatic HTTP download
    // (unlike Safari/Mail) doesn't reliably set com.apple.quarantine
    // either -- but clearing it if present costs nothing and avoids a rare
    // Gatekeeper prompt silently blocking a background process spawn.
    let _ = Command::new("xattr")
        .arg("-dr")
        .arg("com.apple.quarantine")
        .arg(extracted_dir(app_dir))
        .status()
        .await;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&binary_path) {
            let mut perms = meta.permissions();
            perms.set_mode(perms.mode() | 0o111);
            let _ = std::fs::set_permissions(&binary_path, perms);
        }
    }

    if !binary_path.exists() {
        anyhow::bail!(
            "llama-server binary not found after extraction at {}",
            binary_path.display()
        );
    }
    Ok(binary_path)
}

async fn health_check(port: u16) -> bool {
    http_client()
        .get(format!("http://127.0.0.1:{port}/health"))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Spawns `llama-server` for `model_id` and polls `/health` until ready
/// (or `SERVER_STARTUP_TIMEOUT` elapses). Runs as a detached background
/// task — never awaited directly by a request in flight, so a cold model
/// load never blocks any single email's Layer 6 budget.
async fn start_server_task(app_dir: PathBuf, model_id: String) {
    let outcome: Result<Child> = async {
        let binary = ensure_binary(&app_dir).await?;
        let model_path = crate::llm_manager::get_model_path(&app_dir, &model_id)
            .ok_or_else(|| anyhow!("model file not present on disk for {model_id}"))?;

        let mut child = Command::new(&binary)
            .arg("-m")
            .arg(&model_path)
            .arg("--port")
            .arg(LLAMA_SERVER_PORT.to_string())
            .arg("--host")
            .arg("127.0.0.1")
            .arg("-c")
            .arg(CONTEXT_SIZE)
            // CPU-only -- matches the RAM-only (no VRAM) hardware-eligibility
            // gate this catalog's tiers are already defined against
            // (Doc 16 §12.3), and the original Candle path's own express
            // "safest fallback" framing (extraction/llm.rs's prior
            // run_inference: "Setup CPU Device (safest fallback)").
            .arg("-ngl")
            .arg("0")
            .kill_on_drop(true)
            .spawn()
            .context("failed to spawn llama-server")?;

        let deadline = Instant::now() + SERVER_STARTUP_TIMEOUT;
        loop {
            if let Ok(Some(exit_status)) = child.try_wait() {
                anyhow::bail!("llama-server exited during startup: {exit_status}");
            }
            if health_check(LLAMA_SERVER_PORT).await {
                break;
            }
            if Instant::now() > deadline {
                let _ = child.kill().await;
                anyhow::bail!(
                    "llama-server did not become healthy within {SERVER_STARTUP_TIMEOUT:?}"
                );
            }
            tokio::time::sleep(HEALTH_POLL_INTERVAL).await;
        }
        Ok(child)
    }
    .await;

    let mut st = state().lock().await;
    match outcome {
        Ok(child) => {
            st.child = Some(child);
            st.model_id = Some(model_id);
            st.state = ServerState::Ready {
                port: LLAMA_SERVER_PORT,
            };
        }
        Err(e) => {
            tracing::error!("llama-server startup failed: {e}");
            st.state = ServerState::Failed {
                reason: e.to_string(),
                at: Instant::now(),
            };
        }
    }
}

/// Returns the port a healthy, correctly-modeled server is already
/// listening on, or an `Err` describing why not — never blocks waiting for
/// a cold start; kicks one off in the background and reports "not ready
/// yet" immediately so the caller (a single email's Layer 6 attempt) can
/// fail fast rather than hang on someone else's multi-GB model load.
async fn ensure_server_ready(app_dir: &Path, model_id: &str) -> Result<u16> {
    let mut st = state().lock().await;
    match &st.state {
        ServerState::Ready { port } if st.model_id.as_deref() == Some(model_id) => Ok(*port),
        ServerState::Ready { .. } => {
            if let Some(mut child) = st.child.take() {
                let _ = child.kill().await;
            }
            st.state = ServerState::Starting;
            tokio::spawn(start_server_task(
                app_dir.to_path_buf(),
                model_id.to_string(),
            ));
            Err(anyhow!(
                "llama-server restarting for a model change — try again shortly"
            ))
        }
        ServerState::Starting => Err(anyhow!("llama-server still starting — try again shortly")),
        ServerState::Failed { reason, at } if at.elapsed() < FAILURE_COOLDOWN => {
            Err(anyhow!("llama-server previously failed to start: {reason}"))
        }
        ServerState::NotStarted | ServerState::Failed { .. } => {
            st.state = ServerState::Starting;
            tokio::spawn(start_server_task(
                app_dir.to_path_buf(),
                model_id.to_string(),
            ));
            Err(anyhow!("llama-server starting — try again shortly"))
        }
    }
}

#[derive(serde::Deserialize)]
struct CompletionResponse {
    content: String,
}

/// Runs one completion against the warm sidecar server, starting it first
/// if necessary. The caller (`LlmEngine::extract`) wraps this whole call in
/// its own `INFERENCE_TIMEOUT`; only the actual HTTP request below counts
/// against that budget — `ensure_server_ready` itself never blocks on a
/// cold start.
pub async fn complete(app_dir: &Path, model_id: &str, prompt: &str) -> Result<String> {
    let port = ensure_server_ready(app_dir, model_id).await?;

    let resp = http_client()
        .post(format!("http://127.0.0.1:{port}/completion"))
        .json(&serde_json::json!({
            "prompt": prompt,
            "n_predict": 256,
            "temperature": 0.0,
            "stream": false,
        }))
        .send()
        .await
        .context("llama-server /completion request failed")?
        .error_for_status()
        .context("llama-server /completion returned an error status")?;

    let parsed: CompletionResponse = resp
        .json()
        .await
        .context("llama-server /completion response was not the expected JSON shape")?;
    Ok(parsed.content)
}
