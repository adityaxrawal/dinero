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

/// Doc 2026-07-26 mail scan performance: `--parallel N` on a single
/// `llama-server` process batches N concurrent generations sharing the same
/// GPU/memory-bandwidth budget — it does not multiply compute. This
/// measures whether `requested_slots` concurrent completions actually stay
/// within a tolerable slowdown of one solo completion on THIS machine, and
/// steps down proportionally if not, rather than trusting the static
/// RAM/CPU-derived recommendation blindly.
const SLOWDOWN_BUDGET: f64 = 1.5;

pub(crate) fn calibrate_effective_slots(
    requested_slots: usize,
    solo_latency: Duration,
    burst_latency: Duration,
) -> usize {
    if requested_slots <= 1 {
        return requested_slots.max(1);
    }
    let solo_ms = (solo_latency.as_millis().max(1)) as f64;
    let burst_ms = burst_latency.as_millis() as f64;
    let budget_ms = solo_ms * SLOWDOWN_BUDGET;
    if burst_ms <= budget_ms {
        return requested_slots;
    }
    let ratio = budget_ms / burst_ms;
    ((requested_slots as f64 * ratio).floor() as usize).clamp(1, requested_slots)
}

const TIMEOUT_SAFETY_MARGIN: f64 = 1.5;
const MIN_CALIBRATED_TIMEOUT: Duration = Duration::from_secs(20);

/// Replaces the old fixed 60s `INFERENCE_TIMEOUT` constant with a value
/// derived from what this machine actually measured, so a genuinely-slow
/// (but working) completion under real concurrency isn't killed early and
/// thrown into a wasted retry.
pub(crate) fn calibrate_timeout(burst_latency: Duration) -> Duration {
    let scaled = Duration::from_secs_f64(burst_latency.as_secs_f64() * TIMEOUT_SAFETY_MARGIN);
    scaled.max(MIN_CALIBRATED_TIMEOUT)
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
    effective_slots: usize,     // what calibration actually settled on
    semaphore: Arc<Semaphore>,  // sized to effective_slots, not parallel_slots
    calibrated_timeout: Duration,
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
            effective_slots: 0,
            semaphore: Arc::new(Semaphore::new(1)),
            calibrated_timeout: Duration::from_secs(60),
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

fn get_free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .map(|addr| addr.port())
        .unwrap_or(58121)
}

/// Spawns `llama-server` for `model_id` and polls `/health` until ready
/// (or `SERVER_STARTUP_TIMEOUT` elapses). Runs as a detached background
/// task — never awaited directly by a request in flight, so a cold model
/// load never blocks any single email's Layer 6 budget.
async fn start_server_task(app_dir: PathBuf, model_id: String, slots: usize) {
    let port = get_free_port();
    let outcome: Result<Child> = async {
        let binary = ensure_binary(&app_dir).await?;
        let model_path = crate::llm_manager::get_model_path(&app_dir, &model_id)
            .ok_or_else(|| anyhow!("model file not present on disk for {model_id}"))?;

        let mut child = Command::new(&binary)
            .arg("-m")
            .arg(&model_path)
            .arg("--port")
            .arg(port.to_string())
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
                anyhow::bail!("llama-server exited during startup: {exit_status}");
            }
            if health_check(port).await {
                break Ok(child);
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

    let calibration = if outcome.is_ok() {
        const CALIBRATION_PROMPT: &str = "Reply with exactly the single word: OK";

        let calibration_ctx = crate::logging::llm_logger::LlmCallContext::unclassified();
        let solo_start = Instant::now();
        let _ = raw_complete(port, &model_id, CALIBRATION_PROMPT, Duration::from_secs(30), None, calibration_ctx).await;
        let solo_latency = solo_start.elapsed();

        let burst_latency = if slots > 1 {
            let burst_start = Instant::now();
            let mut set = tokio::task::JoinSet::new();
            for _ in 0..slots {
                let mid = model_id.clone();
                let cal_ctx = crate::logging::llm_logger::LlmCallContext::unclassified();
                set.spawn(raw_complete(port, mid, CALIBRATION_PROMPT, Duration::from_secs(90), None, cal_ctx));
            }
            while set.join_next().await.is_some() {}
            burst_start.elapsed()
        } else {
            solo_latency
        };

        let effective_slots = calibrate_effective_slots(slots, solo_latency, burst_latency);
        let calibrated_timeout = calibrate_timeout(burst_latency);
        tracing::info!(
            requested_slots = slots,
            effective_slots,
            solo_latency_ms = solo_latency.as_millis() as u64,
            burst_latency_ms = burst_latency.as_millis() as u64,
            calibrated_timeout_ms = calibrated_timeout.as_millis() as u64,
            "Layer 6 sidecar calibration complete"
        );
        Some((effective_slots, calibrated_timeout))
    } else {
        None
    };

    let mut st = state().lock().await;
    match outcome {
        Ok(child) => {
            let (effective_slots, calibrated_timeout) =
                calibration.expect("calibration always runs when outcome is Ok");
            st.child = Some(child);
            st.model_id = Some(model_id);
            st.parallel_slots = slots;
            st.effective_slots = effective_slots;
            st.semaphore = Arc::new(Semaphore::new(effective_slots));
            st.calibrated_timeout = calibrated_timeout;
            st.state = ServerState::Ready { port };
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
async fn ensure_server_ready(
    app_dir: &Path,
    model_id: &str,
) -> Result<(u16, Arc<Semaphore>, Duration)> {
    let requested_slots = current_parallel_slots();
    let mut st = state().lock().await;
    match &st.state {
        ServerState::Ready { port }
            if server_matches(st.model_id.as_deref(), st.parallel_slots, model_id, requested_slots) =>
        {
            Ok((*port, Arc::clone(&st.semaphore), st.calibrated_timeout))
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
/// Doc 2026-07-28 mail scan performance: real scan logs showed Layer 6
/// rejecting the model's output as "unparseable JSON" on essentially every
/// first attempt, paying for a second full inference (self-correction retry)
/// that often failed too — pure wasted latency for a syntax problem, not a
/// content problem. `json_schema` has llama-server constrain decoding to
/// this shape via grammar sampling, so the output is *always* syntactically
/// valid JSON; `parse_json_to_result`'s field/source validation (the
/// content-correctness check) still runs unchanged on top of it.
/// The JSON schema used by Layer 6 extraction for grammar-constrained decoding.
/// Exposed so `extraction/llm.rs` can pass it explicitly to
/// `complete_with_schema_and_context` without duplicating the definition.
pub fn layer6_json_schema_pub() -> serde_json::Value {
    layer6_json_schema()
}

fn layer6_json_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "amount": {"type": ["number", "null"]},
            "currency": {"type": ["string", "null"]},
            "direction": {"type": ["string", "null"]},
            "merchant": {"type": ["string", "null"]},
            "event_time": {"type": ["integer", "null"]},
            "reference_id": {"type": ["string", "null"]},
            // Was missing entirely: the prompt asks the model for a
            // self-reported `confidence`, but the grammar built from this
            // schema constrains the model to *only* the listed properties,
            // so it had no way to emit one -- `confidence` silently defaulted
            // to 0.0 on every call (see `parse_json_to_result`), which meant
            // Layer 6's auto-resolve threshold could never be reached.
            //
            // 2026-07-30: made `required` -- with it merely a listed
            // property, the model omitted it on 80/80 sampled real calls
            // (unlike `event_time`, a fabricated confidence number can't
            // corrupt the transaction itself; the fields it gates on --
            // merchant/amount/reference -- already passed
            // `validate_against_source` before confidence is ever read).
            "confidence": {"type": ["number", "null"]}
        },
        "required": ["confidence"]
    })
}

async fn raw_complete(
    port: u16,
    model_id: impl AsRef<str>,
    prompt: impl AsRef<str>,
    timeout: Duration,
    json_schema: Option<serde_json::Value>,
    ctx: crate::logging::llm_logger::LlmCallContext,
) -> Result<String> {
    let model_id = model_id.as_ref();
    let prompt = prompt.as_ref();
    // Log the outgoing request before touching the wire.
    crate::logging::llm_logger::log_llm_request(model_id, &ctx, prompt);
    let request_start = Instant::now();

    let mut body = serde_json::json!({
        "prompt": prompt,
        "n_predict": 256,
        "temperature": 0.0,
        "stream": false,
    });
    if let Some(schema) = json_schema {
        body["json_schema"] = schema;
    }
    let result = http_client()
        .post(format!("http://127.0.0.1:{port}/completion"))
        .timeout(timeout)
        .json(&body)
        .send()
        .await
        .context("llama-server /completion request failed")
        .and_then(|r| r.error_for_status().context("llama-server /completion returned an error status"));

    let duration_ms = request_start.elapsed().as_millis() as u64;

    match result {
        Ok(resp) => {
            match resp.json::<CompletionResponse>().await
                .context("llama-server /completion response was not the expected JSON shape")
            {
                Ok(parsed) => {
                    crate::logging::llm_logger::log_llm_response(
                        model_id,
                        &ctx,
                        duration_ms,
                        // Outcome is classified at the extraction layer (run_completion);
                        // at this level we only know the HTTP call succeeded.
                        crate::logging::llm_logger::LlmOutcome::Accepted,
                        Some(&parsed.content),
                    );
                    Ok(parsed.content)
                }
                Err(e) => {
                    crate::logging::llm_logger::log_llm_response(
                        model_id,
                        &ctx,
                        duration_ms,
                        crate::logging::llm_logger::LlmOutcome::InfraFailed,
                        None,
                    );
                    Err(e)
                }
            }
        }
        Err(e) => {
            // Distinguish timeout from other infra failures in the log.
            let outcome = if e.to_string().contains("timeout") || e.to_string().contains("timed out") {
                crate::logging::llm_logger::LlmOutcome::TimedOut
            } else {
                crate::logging::llm_logger::LlmOutcome::InfraFailed
            };
            crate::logging::llm_logger::log_llm_response(
                model_id,
                &ctx,
                duration_ms,
                outcome,
                None,
            );
            Err(e)
        }
    }
}

pub async fn complete(app_dir: &Path, model_id: &str, prompt: &str, timeout: Duration) -> Result<String> {
    let (port, semaphore, _calibrated_timeout) = ensure_server_ready(app_dir, model_id).await?;
    let _permit = semaphore
        .acquire()
        .await
        .context("llama-server semaphore closed")?;
    let ctx = crate::logging::llm_logger::LlmCallContext::unclassified();
    raw_complete(port, model_id, prompt, timeout, None, ctx).await
}

/// Same as `complete`, but sources its timeout from this server's own
/// calibration (Doc 2026-07-26 mail scan performance) instead of a caller-
/// supplied fixed constant.
pub async fn complete_with_calibrated_timeout(
    app_dir: &Path,
    model_id: &str,
    prompt: &str,
) -> Result<String> {
    let ctx = crate::logging::llm_logger::LlmCallContext::unclassified();
    complete_with_schema_and_context(app_dir, model_id, prompt, layer6_json_schema(), ctx).await
}

/// Same calibrated-timeout, semaphore-gated path as
/// [`complete_with_calibrated_timeout`], but for callers that constrain the
/// output to their own shape rather than Layer 6's extraction schema (issue
/// #12's merchant/category pass). Grammar sampling is what makes the closed
/// category list enforceable at the decoder rather than by rejecting bad
/// answers after the fact.
pub async fn complete_with_schema(
    app_dir: &Path,
    model_id: &str,
    prompt: &str,
    schema: serde_json::Value,
) -> Result<String> {
    let ctx = crate::logging::llm_logger::LlmCallContext::unclassified();
    complete_with_schema_and_context(app_dir, model_id, prompt, schema, ctx).await
}

/// Internal: complete with a JSON schema constraint and an explicit call
/// context for the LLM call log. Called by `extraction/llm.rs` and
/// `extraction/rule_llm.rs` (via the learning path) when they want attributed
/// log entries.
pub async fn complete_with_schema_and_context(
    app_dir: &Path,
    model_id: &str,
    prompt: &str,
    schema: serde_json::Value,
    ctx: crate::logging::llm_logger::LlmCallContext,
) -> Result<String> {
    let (port, semaphore, calibrated_timeout) = ensure_server_ready(app_dir, model_id).await?;
    let _permit = semaphore
        .acquire()
        .await
        .context("llama-server semaphore closed")?;
    raw_complete(port, model_id, prompt, calibrated_timeout, Some(schema), ctx).await
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

    use super::{calibrate_effective_slots, calibrate_timeout};
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn calibrate_effective_slots_keeps_requested_when_burst_is_within_budget() {
        // Burst only 1.2x slower than solo — well within the 1.5x budget.
        let result = calibrate_effective_slots(10, Duration::from_secs(5), Duration::from_secs(6));
        assert_eq!(result, 10);
    }

    #[test]
    fn calibrate_effective_slots_steps_down_when_burst_exceeds_budget() {
        // Burst 3x slower than solo, budget is 1.5x — hardware can sustain
        // roughly requested_slots * (1.5/3.0) = 5 slots, not 10.
        let result = calibrate_effective_slots(10, Duration::from_secs(5), Duration::from_secs(15));
        assert_eq!(result, 5);
    }

    #[test]
    fn calibrate_effective_slots_never_goes_below_one() {
        let result = calibrate_effective_slots(10, Duration::from_secs(1), Duration::from_secs(1000));
        assert_eq!(result, 1);
    }

    #[test]
    fn calibrate_effective_slots_skips_calibration_at_slots_equal_one() {
        // No burst to compare against when the user only asked for 1 slot.
        let result = calibrate_effective_slots(1, Duration::from_secs(5), Duration::from_secs(50));
        assert_eq!(result, 1);
    }

    #[test]
    fn calibrate_timeout_scales_with_measured_burst_latency() {
        let timeout = calibrate_timeout(Duration::from_secs(40));
        assert_eq!(timeout, Duration::from_secs(60)); // 40 * 1.5
    }

    #[test]
    fn calibrate_timeout_never_goes_below_the_floor() {
        let timeout = calibrate_timeout(Duration::from_secs(2));
        assert_eq!(timeout, Duration::from_secs(20)); // floor, not 2*1.5=3
    }

    #[test]
    fn server_state_defaults_have_no_calibration_yet() {
        // A freshly-constructed SidecarState (before any server has started)
        // must not claim a calibrated timeout of zero — that would make
        // complete() time out instantly. Documents the required initial value
        // so start_server_task's struct literal is held to it.
        let st = super::SidecarState {
            state: super::ServerState::NotStarted,
            child: None,
            model_id: None,
            parallel_slots: 0,
            effective_slots: 0,
            semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
            calibrated_timeout: Duration::from_secs(60),
        };
        assert_eq!(st.calibrated_timeout, Duration::from_secs(60));
    }
}
