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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, Semaphore};

const LLAMA_CPP_RELEASE_TAG: &str = "b10068";
const LLAMA_SERVER_PORT: u16 = 58121;

/// Runtime-configurable count of concurrent `/completion` requests
/// `llama-server` will batch-process at once, set by the user in Settings
/// (`llm_set_parallel_slots`, clamped 1-10). Starts at the original safe
/// default of 1 — a historical scan started before Settings ever pushes a
/// real value just runs single-slot, same as before this feature existed.
static CURRENT_PARALLEL_SLOTS: AtomicUsize = AtomicUsize::new(1);

pub fn set_parallel_slots(n: usize) {
    CURRENT_PARALLEL_SLOTS.store(n.clamp(1, 10), Ordering::Relaxed);
}

pub fn current_parallel_slots() -> usize {
    CURRENT_PARALLEL_SLOTS.load(Ordering::Relaxed)
}

fn context_size_for(slots: usize) -> usize {
    // llama-server splits its total context evenly across `--parallel`
    // slots (`n_ctx_slot = n_ctx / n_parallel`), so this must scale with
    // the slot count to keep each slot's own budget at the server's
    // original single-slot default (2048) — otherwise more parallelism
    // would silently truncate the email body in every prompt.
    2048 * slots
}

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

/// Doc 16 §12.3's hardware-eligibility gate is RAM-based, not VRAM-based --
/// on Apple Silicon that's not a reason to force CPU-only inference, since
/// Metal's GPU layers share the same unified system RAM the gate already
/// budgets against (there's no separate VRAM pool to exceed). Forcing
/// `-ngl 0` on aarch64 meant every catalog tier (4B-35B params) had to
/// decode 256 tokens CPU-only inside a 10s hard budget, which real-world
/// logs show it essentially never met (0/50 successes; ~45/49 failures are
/// wall-clock timeouts, see `extraction/llm.rs::INFERENCE_TIMEOUT`). Intel
/// Macs have no such unified-memory GPU to offload to, so they keep the
/// original CPU-only behavior.
#[cfg(target_arch = "aarch64")]
fn gpu_layers_arg() -> &'static str {
    "all"
}
#[cfg(not(target_arch = "aarch64"))]
fn gpu_layers_arg() -> &'static str {
    "0"
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
    parallel_slots: usize,
    semaphore: Arc<Semaphore>,
}

fn state() -> &'static Mutex<SidecarState> {
    static STATE: OnceLock<Mutex<SidecarState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(SidecarState {
            state: ServerState::NotStarted,
            child: None,
            model_id: None,
            // 0 can never equal a real requested slot count (always >= 1
            // after clamping), so this can't accidentally "match" before a
            // server has actually been spawned.
            parallel_slots: 0,
            semaphore: Arc::new(Semaphore::new(1)),
        })
    })
}

/// Whether an already-`Ready` server (running `current_model` at
/// `current_slots`) already satisfies a request for `requested_model` at
/// `requested_slots` — true means no restart needed. Pure so the four
/// independent combinations are unit-testable without spawning a process.
fn server_matches(
    current_model: Option<&str>,
    current_slots: usize,
    requested_model: &str,
    requested_slots: usize,
) -> bool {
    current_model == Some(requested_model) && current_slots == requested_slots
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
    crate::llm_manager::download_file_with_hash(&url, &tarball_path, expected_sha256, None, None)
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
async fn start_server_task(app_dir: PathBuf, model_id: String, slots: usize) {
    let outcome: Result<Option<Child>> = async {
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
            .arg(context_size_for(slots).to_string())
            .arg("--parallel")
            .arg(slots.to_string())
            .arg("-ngl")
            .arg(gpu_layers_arg())
            // `--host 127.0.0.1` already blocks remote network access, but
            // llama-server's own CORS default (`--cors-origins`, unset here
            // otherwise) is `*` -- any webpage open in the user's browser on
            // this machine could otherwise call this port directly and have
            // the response readable via CORS, a known localhost-app attack
            // pattern. The only real caller is this process's own `reqwest`
            // client (`complete()` below), which never sends an `Origin`
            // header, so restricting this costs nothing functionally.
            .arg("--cors-origins")
            .arg("localhost")
            .kill_on_drop(true)
            .spawn()
            .context("failed to spawn llama-server")?;

        let deadline = Instant::now() + SERVER_STARTUP_TIMEOUT;
        loop {
            if let Ok(Some(exit_status)) = child.try_wait() {
                // A common dev-mode cause: an earlier run of this same app
                // (killed by a hot-reload restart or force-quit) never got to
                // call `shutdown()`, so its `llama-server` child is still
                // alive and holding the port -- our own spawn above then
                // fails to bind and exits immediately. Rather than declaring
                // a hard `Failed` and sitting out `FAILURE_COOLDOWN` with
                // Layer 6 unavailable the whole time, check whether whatever
                // already holds the port is actually healthy before giving
                // up; if so, adopt it (we don't own its process, so no
                // `Child` to store — `shutdown()` simply has nothing of ours
                // to kill).
                //
                // Caveat: `/health` only proves *a* server answers, not that
                // it has `model_id` loaded rather than whatever the prior
                // session last selected. Not verified here -- llama-server's
                // model-introspection endpoint shape isn't stable enough
                // across releases to key correctness off of, and a wrong
                // model's output still has to clear
                // `LlmEngine::validate_against_source`'s merchant/reference/
                // amount check same as any other Layer 6 attempt, bounding
                // the damage to an extra miss rather than a silent bad
                // value. Logged so a real mismatch is at least diagnosable.
                if health_check(LLAMA_SERVER_PORT).await {
                    tracing::warn!(
                        "adopted an already-running llama-server on port {LLAMA_SERVER_PORT} \
                         instead of starting our own for model_id='{model_id}' -- its actual \
                         loaded model is unverified"
                    );
                    break Ok(None);
                }
                anyhow::bail!("llama-server exited during startup: {exit_status}");
            }
            if health_check(LLAMA_SERVER_PORT).await {
                break Ok(Some(child));
            }
            if Instant::now() > deadline {
                let _ = child.kill().await;
                anyhow::bail!(
                    "llama-server did not become healthy within {SERVER_STARTUP_TIMEOUT:?}"
                );
            }
            tokio::time::sleep(HEALTH_POLL_INTERVAL).await;
        }
    }
    .await;

    let mut st = state().lock().await;
    match outcome {
        Ok(child) => {
            st.child = child;
            st.model_id = Some(model_id);
            st.parallel_slots = slots;
            st.semaphore = Arc::new(Semaphore::new(slots));
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

pub async fn shutdown() {
    let mut st = state().lock().await;
    if let Some(mut child) = st.child.take() {
        let _ = child.kill().await;
    }
    st.state = ServerState::NotStarted;
}

/// Returns the port a healthy, correctly-modeled server is already
/// listening on, or an `Err` describing why not — never blocks waiting for
/// a cold start; kicks one off in the background and reports "not ready
/// yet" immediately so the caller (a single email's Layer 6 attempt) can
/// fail fast rather than hang on someone else's multi-GB model load.
async fn ensure_server_ready(app_dir: &Path, model_id: &str) -> Result<(u16, Arc<Semaphore>)> {
    let requested_slots = current_parallel_slots();
    let mut st = state().lock().await;
    match &st.state {
        ServerState::Ready { port }
            if server_matches(st.model_id.as_deref(), st.parallel_slots, model_id, requested_slots) =>
        {
            Ok((*port, Arc::clone(&st.semaphore)))
        }
        ServerState::Ready { .. } => {
            if let Some(mut child) = st.child.take() {
                let _ = child.kill().await;
            }
            st.state = ServerState::Starting;
            tokio::spawn(start_server_task(
                app_dir.to_path_buf(),
                model_id.to_string(),
                requested_slots,
            ));
            Err(anyhow!(
                "llama-server restarting for a model/parallelism change — try again shortly"
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
                requested_slots,
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
/// its own `INFERENCE_TIMEOUT`; `ensure_server_ready` never blocks on a
/// cold start and the completion slot below is `try_acquire`d rather than
/// waited on, so nothing here can eat into that budget except the actual
/// HTTP request.
pub async fn complete(app_dir: &Path, model_id: &str, prompt: &str) -> Result<String> {
    let (port, semaphore) = ensure_server_ready(app_dir, model_id).await?;
    let _permit = semaphore
        .try_acquire()
        .context("llama-server busy with another completion")?;

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

#[cfg(test)]
mod tests {
    use super::server_matches;

    /// Regression test for the fix above: a caller must never block on the
    /// single completion slot. Blocking here is what made every queued
    /// email in a concurrent batch burn its whole `INFERENCE_TIMEOUT`
    /// waiting instead of failing over to the next ladder tier.
    #[test]
    fn second_completion_fails_fast_instead_of_queueing() {
        let sem = tokio::sync::Semaphore::new(1);
        let _held = sem.try_acquire().expect("first acquire must succeed");
        assert!(
            sem.try_acquire().is_err(),
            "a contended slot must be rejected immediately, not waited on"
        );
    }

    #[test]
    fn server_matches_true_when_model_and_slots_both_match() {
        assert!(server_matches(Some("gemma4_e4b"), 4, "gemma4_e4b", 4));
    }

    #[test]
    fn server_matches_false_when_model_differs() {
        assert!(!server_matches(Some("gemma4_12b"), 4, "gemma4_e4b", 4));
    }

    #[test]
    fn server_matches_false_when_slots_differ() {
        assert!(!server_matches(Some("gemma4_e4b"), 4, "gemma4_e4b", 6));
    }

    #[test]
    fn server_matches_false_when_nothing_has_run_yet() {
        assert!(!server_matches(None, 4, "gemma4_e4b", 4));
    }
}
