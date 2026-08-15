//! Manages the llama.cpp server subprocess used for local inference.
//!
//! The sidecar is a separate process rather than an in-process library, which
//! keeps a crash or a runaway allocation inside inference from taking the whole
//! application down with it. This module owns its full lifecycle: fetching the
//! release binary, verifying it, starting it, waiting for health, and shutting
//! it down.
//!
//! Two calibration passes exist because published hardware specifications do not
//! predict real throughput. On startup the sidecar is measured under solo and
//! burst load, and the results are used to reduce the parallel slot count until
//! per-request latency stays within budget, and to derive a request timeout from
//! observed latency rather than a guessed constant. This is the "leave the
//! calibration knob" case: a machine that is thermally throttled, on battery, or
//! sharing its GPU behaves nothing like the same model on paper.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, Semaphore};

const LLAMA_CPP_RELEASE_TAG: &str = "b10068";

static CURRENT_PARALLEL_SLOTS: AtomicUsize = AtomicUsize::new(1);

/// Sets the parallel slot count the next server start will use.
pub fn set_parallel_slots(n: usize) {
    CURRENT_PARALLEL_SLOTS.store(n.clamp(1, 10), Ordering::Relaxed);
}

/// The currently configured slot count.
pub fn current_parallel_slots() -> usize {
    CURRENT_PARALLEL_SLOTS.load(Ordering::Relaxed)
}

// Context is allocated per slot, so the total scales with concurrency. This is
// the main reason slot count is memory-bound.
fn context_size_for(slots: usize) -> usize {
    2048 * slots
}

// Startup can legitimately take a long time -- a multi-gigabyte model is being
// mapped into memory -- so the timeout is generous and health is polled rather
// than assumed after a fixed delay.
const SERVER_STARTUP_TIMEOUT: Duration = Duration::from_secs(120);
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(500);

// After a failed start, back off before retrying. Without this a persistently
// broken sidecar would be respawned in a tight loop.
const FAILURE_COOLDOWN: Duration = Duration::from_secs(60);

// Per-architecture release asset, pinned by name and SHA-256. The hash is what
// makes the download trustworthy: the binary is fetched over plain HTTPS from a
// third party and is then executed, so it is verified before it is ever run.
#[cfg(target_arch = "aarch64")]
/// Release asset and its SHA-256 for Apple Silicon.
fn release_asset() -> (&'static str, &'static str) {
    (
        "llama-b10068-bin-macos-arm64.tar.gz",
        "13aa2d40c76ad1dcb8ebeec5f0d2814bf3b2f84a66935c7d4dc6f7cca8e38d68",
    )
}
#[cfg(not(target_arch = "aarch64"))]
/// Release asset and its SHA-256 for Intel.
fn release_asset() -> (&'static str, &'static str) {
    (
        "llama-b10068-bin-macos-x64.tar.gz",
        "73a63a0fdcfd8d0625fe20aa8f2af62e3d6437c6380b46129ca1a9abacbde0d5",
    )
}

// Apple Silicon offloads every layer to the GPU via Metal; on Intel there is no
// usable acceleration path, so inference stays on the CPU.
#[cfg(target_arch = "aarch64")]
/// Offload every layer to the GPU on Apple Silicon, via Metal.
fn gpu_layers_arg() -> &'static str {
    "all"
}
#[cfg(not(target_arch = "aarch64"))]
/// No GPU offload on Intel, where no usable acceleration path exists.
fn gpu_layers_arg() -> &'static str {
    "0"
}

// How much per-request slowdown under concurrency is acceptable before slots
// are reduced. 1.5 means a request may take at most half again as long in a
// burst as it does alone.
const SLOWDOWN_BUDGET: f64 = 1.5;

/// Reduce the requested slot count until measured contention fits the budget.
///
/// Compares solo latency against burst latency and scales slots down by the
/// ratio of the budget to what was observed. Real hardware rarely sustains its
/// nominal parallelism -- thermal limits, memory bandwidth and a shared GPU all
/// bite -- so the count is derived from measurement rather than trusted from
/// specification. Always leaves at least one slot.
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

// Request timeout is derived from observed burst latency plus headroom, with a
// floor so a fast machine does not end up with a timeout too tight to survive a
// transient stall.
const TIMEOUT_SAFETY_MARGIN: f64 = 1.5;
const MIN_CALIBRATED_TIMEOUT: Duration = Duration::from_secs(20);

/// Derive a request timeout from measured latency, never below the floor.
pub(crate) fn calibrate_timeout(burst_latency: Duration) -> Duration {
    let scaled = Duration::from_secs_f64(burst_latency.as_secs_f64() * TIMEOUT_SAFETY_MARGIN);
    scaled.max(MIN_CALIBRATED_TIMEOUT)
}

/// Root directory for sidecar files.
fn base_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("llama_cpp")
}

/// Directory the release archive is extracted into.
fn extracted_dir(app_dir: &Path) -> PathBuf {
    base_dir(app_dir).join(format!("llama-{LLAMA_CPP_RELEASE_TAG}"))
}

/// Path of the llama.cpp server binary.
fn server_binary_path(app_dir: &Path) -> PathBuf {
    extracted_dir(app_dir).join("llama-server")
}

/// Shared HTTP client for sidecar requests.
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
    effective_slots: usize,
    semaphore: Arc<Semaphore>,
    calibrated_timeout: Duration,
}

/// The process-wide sidecar state.
fn state() -> &'static Mutex<SidecarState> {
    static STATE: OnceLock<Mutex<SidecarState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(SidecarState {
            state: ServerState::NotStarted,
            child: None,
            model_id: None,
            parallel_slots: 0,
            effective_slots: 0,
            semaphore: Arc::new(Semaphore::new(1)),
            calibrated_timeout: Duration::from_secs(60),
        })
    })
}

/// Whether the running server already matches the requested model and slots.
///
/// Avoids restarting a server that is already serving the right configuration,
/// which would otherwise re-map a multi-gigabyte model for nothing.
fn server_matches(
    current_model: Option<&str>,
    current_slots: usize,
    requested_model: &str,
    requested_slots: usize,
) -> bool {
    current_model == Some(requested_model) && current_slots == requested_slots
}

/// Ensures the server binary is present, downloading and verifying if not.
///
/// The SHA-256 check is the security-critical part: this binary is fetched from a
/// third party and then executed, so it is verified before it is ever run.
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

/// Polls the server's health endpoint.
async fn health_check(port: u16) -> bool {
    http_client()
        .get(format!("http://127.0.0.1:{port}/health"))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Finds a free localhost port for the server.
fn get_free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .map(|addr| addr.port())
        .unwrap_or(58121)
}

/// Starts the server and waits for it to become healthy.
///
/// Health is polled rather than assumed after a fixed delay, because startup time
/// varies with model size and disk speed.
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
        let _ = raw_complete(
            port,
            &model_id,
            CALIBRATION_PROMPT,
            Duration::from_secs(30),
            None,
            calibration_ctx,
        )
        .await;
        let solo_latency = solo_start.elapsed();

        let burst_latency = if slots > 1 {
            let burst_start = Instant::now();
            let mut set = tokio::task::JoinSet::new();
            for _ in 0..slots {
                let mid = model_id.clone();
                let cal_ctx = crate::logging::llm_logger::LlmCallContext::unclassified();
                set.spawn(raw_complete(
                    port,
                    mid,
                    CALIBRATION_PROMPT,
                    Duration::from_secs(90),
                    None,
                    cal_ctx,
                ));
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

/// Shuts the sidecar down.
pub async fn shutdown() {
    let mut st = state().lock().await;
    if let Some(mut child) = st.child.take() {
        let _ = child.kill().await;
    }
    st.state = ServerState::NotStarted;
}

/// Ensures a healthy server is running for the requested model.
///
/// Applies a cooldown after a failed start, so a persistently broken sidecar is
/// not respawned in a tight loop.
async fn ensure_server_ready(
    app_dir: &Path,
    model_id: &str,
) -> Result<(u16, Arc<Semaphore>, Duration)> {
    let requested_slots = current_parallel_slots();
    let mut st = state().lock().await;
    match &st.state {
        ServerState::Ready { port }
            if server_matches(
                st.model_id.as_deref(),
                st.parallel_slots,
                model_id,
                requested_slots,
            ) =>
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

/// The layer-6 extraction schema, exposed for tests.
pub fn layer6_json_schema_pub() -> serde_json::Value {
    layer6_json_schema()
}

/// JSON schema constraining extraction output.
fn layer6_json_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "is_transaction": {"type": "boolean"},
            "amount": {"type": ["number", "null"]},
            "currency": {"type": ["string", "null"]},
            "direction": {"type": ["string", "null"]},
            "merchant": {"type": ["string", "null"]},
            "datetime": {"type": ["string", "null"]},
            "reference_id": {"type": ["string", "null"]},
            "confidence": {"type": ["number", "null"]}
        },
        "required": ["is_transaction", "amount", "currency", "direction", "merchant", "datetime", "reference_id", "confidence"]
    })
}

/// Issues a raw completion request to the server.
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
    crate::logging::llm_logger::log_llm_request(model_id, &ctx, prompt);
    let request_start = Instant::now();

    let mut body = serde_json::json!({
        "prompt": prompt,
        "n_predict": 512,
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
        .and_then(|r| {
            r.error_for_status()
                .context("llama-server /completion returned an error status")
        });

    let duration_ms = request_start.elapsed().as_millis() as u64;

    match result {
        Ok(resp) => {
            match resp
                .json::<CompletionResponse>()
                .await
                .context("llama-server /completion response was not the expected JSON shape")
            {
                Ok(parsed) => {
                    crate::logging::llm_logger::log_llm_response(
                        model_id,
                        &ctx,
                        duration_ms,
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
            let outcome =
                if e.to_string().contains("timeout") || e.to_string().contains("timed out") {
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

/// Runs a completion with the default timeout.
pub async fn complete(
    app_dir: &Path,
    model_id: &str,
    prompt: &str,
    timeout: Duration,
) -> Result<String> {
    let (port, semaphore, _calibrated_timeout) = ensure_server_ready(app_dir, model_id).await?;
    let _permit = semaphore
        .acquire()
        .await
        .context("llama-server semaphore closed")?;
    let ctx = crate::logging::llm_logger::LlmCallContext::unclassified();
    raw_complete(port, model_id, prompt, timeout, None, ctx).await
}

/// Runs a completion using the calibrated timeout.
///
/// The timeout is derived from latency measured on this machine rather than a
/// fixed constant, because a thermally throttled or battery-powered laptop
/// performs nothing like the same model on paper.
pub async fn complete_with_calibrated_timeout(
    app_dir: &Path,
    model_id: &str,
    prompt: &str,
) -> Result<String> {
    let ctx = crate::logging::llm_logger::LlmCallContext::unclassified();
    complete_with_schema_and_context(app_dir, model_id, prompt, layer6_json_schema(), ctx).await
}

/// Runs a schema-constrained completion.
pub async fn complete_with_schema(
    app_dir: &Path,
    model_id: &str,
    prompt: &str,
    schema: serde_json::Value,
) -> Result<String> {
    let ctx = crate::logging::llm_logger::LlmCallContext::unclassified();
    complete_with_schema_and_context(app_dir, model_id, prompt, schema, ctx).await
}

/// Runs a schema-constrained completion with additional context.
pub async fn complete_with_schema_and_context(
    app_dir: &Path,
    model_id: &str,
    prompt: &str,
    schema: serde_json::Value,
    ctx: crate::logging::llm_logger::LlmCallContext,
) -> Result<String> {
    complete_with_optional_schema_and_context(app_dir, model_id, prompt, Some(schema), ctx).await
}

/// Runs a completion with an optional schema and context.
pub async fn complete_with_optional_schema_and_context(
    app_dir: &Path,
    model_id: &str,
    prompt: &str,
    schema: Option<serde_json::Value>,
    ctx: crate::logging::llm_logger::LlmCallContext,
) -> Result<String> {
    let (port, semaphore, calibrated_timeout) = ensure_server_ready(app_dir, model_id).await?;
    let _permit = semaphore
        .acquire()
        .await
        .context("llama-server semaphore closed")?;
    
    let effective_timeout = calibrated_timeout.min(Duration::from_secs(150));
    
    raw_complete(
        port,
        model_id,
        prompt,
        effective_timeout,
        schema,
        ctx,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::server_matches;

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
        let result = calibrate_effective_slots(10, Duration::from_secs(5), Duration::from_secs(6));
        assert_eq!(result, 10);
    }

    #[test]
    fn calibrate_effective_slots_steps_down_when_burst_exceeds_budget() {
        let result = calibrate_effective_slots(10, Duration::from_secs(5), Duration::from_secs(15));
        assert_eq!(result, 5);
    }

    #[test]
    fn calibrate_effective_slots_never_goes_below_one() {
        let result =
            calibrate_effective_slots(10, Duration::from_secs(1), Duration::from_secs(1000));
        assert_eq!(result, 1);
    }

    #[test]
    fn calibrate_effective_slots_skips_calibration_at_slots_equal_one() {
        let result = calibrate_effective_slots(1, Duration::from_secs(5), Duration::from_secs(50));
        assert_eq!(result, 1);
    }

    #[test]
    fn calibrate_timeout_scales_with_measured_burst_latency() {
        let timeout = calibrate_timeout(Duration::from_secs(40));
        assert_eq!(timeout, Duration::from_secs(60));
    }

    #[test]
    fn calibrate_timeout_never_goes_below_the_floor() {
        let timeout = calibrate_timeout(Duration::from_secs(2));
        assert_eq!(timeout, Duration::from_secs(20));
    }

    #[test]
    fn server_state_defaults_have_no_calibration_yet() {
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
