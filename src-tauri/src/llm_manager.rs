use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tauri::{AppHandle, Emitter};
use tokio::fs::{self, File};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

/// Emitted to the frontend while a catalog model's `.gguf` downloads, so
/// Settings' model picker can show a real progress bar instead of an
/// indeterminate spinner for what's often a multi-GB, multi-minute
/// download. `total_bytes` is `None` on the rare server that omits
/// `Content-Length` — the frontend falls back to indeterminate in that case.
#[derive(Debug, Clone, Serialize)]
pub struct LlmDownloadProgress {
    pub model_id: String,
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
    /// EMA-smoothed (alpha=0.3) download rate in bytes/sec. `0.0` until the
    /// first throttled emit interval has elapsed.
    pub bytes_per_sec: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmModelInfo {
    pub id: String,
    pub name: String,
    /// Ollama-style tag from Doc 16 §12.3's hardware matrix (e.g. `gemma4:e4b`)
    /// — informational only; `id` (not this) is what's used for file paths.
    pub tag: String,
    pub tier: u8,
    pub min_ram_gb: f64,
    pub approx_size_gb: f64,
    pub rationale: String,
    pub gguf_url: String,
    pub expected_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadState {
    pub model_id: String,
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
    pub status: String, // "downloading", "verifying", "ready", "failed"
}

/// Doc 16 §12.3 "Dinero Local LLM Hardware Matrix" — the single, authoritative
/// 5-tier catalog. Every field except `gguf_url`/`expected_sha256` is taken
/// directly from that table.
///
/// Real artifacts, verified (not fabricated — see the prior revision's own
/// warning about that exact mistake). Each `gguf_url` points at a real
/// HuggingFace `resolve/main` path; each hash is the file's own git-lfs
/// `oid` (HF's LFS storage is content-addressed by SHA-256, so the LFS
/// pointer's `oid` *is* the file's real SHA-256 — read directly via
/// `.../raw/main/<file>`, not computed by downloading and hashing multi-GB
/// files locally). Quantization: Q4_K_M for the four dense tiers (closest
/// available to Doc 16's `approx_size_gb` without materially degrading
/// extraction-task accuracy), MXFP4_MOE for the one MoE tier (Unsloth's own
/// recommended default quant for their MoE conversions). Retrieved
/// 2026-07-19 from the `unsloth/*-GGUF` repos; re-verify if these ever need
/// to change (a repo can rename/retag files upstream).
///
/// No separate `tokenizer_url` — GGUF embeds its own tokenizer vocab/merges
/// (`tokenizer.ggml.*` metadata), and inference runs through `llama_sidecar`
/// (llama.cpp's `llama-server`), which reads that directly. The Candle path
/// this catalog previously targeted needed an external `tokenizer.json`;
/// that requirement went away along with Candle itself (see
/// `llama_sidecar.rs`'s module doc for why: no released `candle-transformers`
/// version has a loader for either of these families' actual GGUF
/// architectures — Gemma 4's own `"gemma4"` tag, Qwen3.6's
/// Gated-DeltaNet-hybrid MoE).
pub fn get_available_models() -> Vec<LlmModelInfo> {
    vec![
        LlmModelInfo {
            id: "gemma4_e4b".to_string(),
            name: "Gemma 4 E4B".to_string(),
            tag: "gemma4:e4b".to_string(),
            tier: 1,
            min_ram_gb: 8.0,
            approx_size_gb: 5.0,
            rationale: "The floor for customers who can't run anything bigger. Native structured JSON output and function calling. Covers the entry-level market.".to_string(),
            gguf_url: "https://huggingface.co/unsloth/gemma-4-E4B-it-GGUF/resolve/main/gemma-4-E4B-it-Q4_K_M.gguf?download=true".to_string(),
            expected_sha256: "85a896a047553e842f25297ee5b031d64ff30147d9c4af17b1e4b394cd1fab87".to_string(),
        },
        LlmModelInfo {
            id: "gemma4_12b".to_string(),
            name: "Gemma 4 12B".to_string(),
            tag: "gemma4:12b".to_string(),
            tier: 2,
            min_ram_gb: 16.0,
            approx_size_gb: 9.0,
            rationale: "The default and sweet spot. Highly capable on 16GB, quality matching models twice its size.".to_string(),
            gguf_url: "https://huggingface.co/unsloth/gemma-4-12b-it-GGUF/resolve/main/gemma-4-12b-it-Q4_K_M.gguf?download=true".to_string(),
            expected_sha256: "0a270ec9fe6b34f4a0d33992b6135117b484ebc4766ab76b51d4ae8c457e4c42".to_string(),
        },
        LlmModelInfo {
            id: "qwen3_6_27b".to_string(),
            name: "Qwen3.6-27B (Dense)".to_string(),
            tag: "qwen3.6:27b".to_string(),
            tier: 3,
            min_ram_gb: 16.0,
            approx_size_gb: 15.0,
            rationale: "Strong alternative/cross-check to Gemma 12B. Dense architecture means it is consistent and predictable for narrow extraction tasks.".to_string(),
            gguf_url: "https://huggingface.co/unsloth/Qwen3.6-27B-GGUF/resolve/main/Qwen3.6-27B-Q4_K_M.gguf?download=true".to_string(),
            expected_sha256: "5ed60d0af4650a854b1755bd392f9aef4872643dc25a254bc68043fa638392a0".to_string(),
        },
        LlmModelInfo {
            id: "qwen3_6_35b_a3b".to_string(),
            name: "Qwen3.6-35B-A3B (MoE)".to_string(),
            tag: "qwen3.6:35b".to_string(),
            tier: 4,
            min_ram_gb: 32.0,
            approx_size_gb: 21.0,
            rationale: "Highest accuracy ceiling realistic for a consumer Mac. MoE makes it fast despite size (70-80 tok/s on MLX). The \"best case\" tier.".to_string(),
            gguf_url: "https://huggingface.co/unsloth/Qwen3.6-35B-A3B-GGUF/resolve/main/Qwen3.6-35B-A3B-MXFP4_MOE.gguf?download=true".to_string(),
            expected_sha256: "2fdd20997c4d88ee25f70f500c61f8b999378d92ab055f9d450fc70d617158d3".to_string(),
        },
        LlmModelInfo {
            id: "gemma4_31b".to_string(),
            name: "Gemma 4 31B (Dense)".to_string(),
            tag: "gemma4:31b".to_string(),
            tier: 5,
            min_ram_gb: 32.0,
            approx_size_gb: 20.0,
            rationale: "Currently the strongest Gemma 4 model. Pick it for maximum quality if memory allows. Keeps prompt engineering consistent with Tier 2.".to_string(),
            gguf_url: "https://huggingface.co/unsloth/gemma-4-31B-it-GGUF/resolve/main/gemma-4-31B-it-Q4_K_M.gguf?download=true".to_string(),
            expected_sha256: "38bd64c852c4b460434cc7162fa9bdcf242faf86502581a754cb72956bb17f84".to_string(),
        },
    ]
}

pub async fn download_model(
    app_dir: &Path,
    model_info: &LlmModelInfo,
    progress_app: Option<&AppHandle>,
    cancel_token: Option<CancellationToken>,
) -> Result<()> {
    // Defensive: catches any future catalog entry that ships without a real
    // artifact yet, same check that caught all 5 entries before this fix.
    if model_info.gguf_url.starts_with("PLACEHOLDER_") {
        anyhow::bail!(
            "Model '{}' has no real download URL configured yet (placeholder artifact, see llm_manager.rs)",
            model_info.id
        );
    }

    let models_dir = app_dir.join("models");
    fs::create_dir_all(&models_dir).await?;

    let model_path = models_dir.join(format!("{}.gguf", model_info.id));

    // Download GGUF — no separate tokenizer download needed, GGUF embeds
    // its own vocab/merges and `llama_sidecar`'s `llama-server` reads that
    // directly.
    download_file_with_hash(
        &model_info.gguf_url,
        &model_path,
        &model_info.expected_sha256,
        progress_app.map(|app| (app, model_info.id.as_str())),
        cancel_token,
    )
    .await?;

    Ok(())
}

/// Deletes a downloaded model and its verification marker from disk.
pub fn delete_model(app_dir: &Path, model_id: &str) -> Result<()> {
    let model_path = app_dir.join("models").join(format!("{}.gguf", model_id));
    let marker_path = verified_marker_path(&model_path);
    let _ = std::fs::remove_file(&model_path);
    let _ = std::fs::remove_file(&marker_path);
    Ok(())
}

/// `progress` — when `Some((app, model_id))`, emits throttled
/// `llm_download_progress` events (~every 1MB, plus a final one) so the
/// frontend can render a real percentage instead of an indeterminate
/// spinner. `None` for callers that don't need progress UI (the
/// `llama_sidecar` binary download, currently).
pub(crate) async fn download_file_with_hash(
    url: &str,
    dest_path: &Path,
    expected_hash: &str,
    progress: Option<(&AppHandle, &str)>,
    cancel_token: Option<CancellationToken>,
) -> Result<()> {
    const PROGRESS_EMIT_INTERVAL_BYTES: u64 = 1_000_000;
    const EMA_ALPHA: f64 = 0.3;

    let client = Client::new();

    // audit_06 #8: resume an interrupted download instead of restarting it.
    // These files are 4–20 GB; on a metered or unstable connection, throwing
    // away 18 GB because the last 2 failed is the difference between "usable"
    // and "not". A partial is left on disk by design — a crash or network drop
    // never reaches the `.verified` marker write below, so `get_model_path`
    // already refuses to hand a partial to `llama-server`.
    let resume_from: u64 = match fs::metadata(dest_path).await {
        Ok(meta) if meta.len() > 0 => meta.len(),
        _ => 0,
    };

    let mut request = client.get(url);
    if resume_from > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={resume_from}-"));
    }
    let mut res = request.send().await?.error_for_status()?;

    // A server that honours the range answers 206 with the remainder. One that
    // ignores it answers 200 with the *whole* file — appending that to our
    // partial would produce a corrupt file that only the final hash check
    // would catch, after another multi-GB download. So trust the status, not
    // the request.
    let resuming = resume_from > 0 && res.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    if resume_from > 0 && !resuming {
        tracing::info!(
            resume_from,
            status = %res.status(),
            "server did not honour Range — restarting the model download from zero"
        );
    }

    // `content_length()` is the length of *this response*, so on a resume it
    // is the remainder, not the file. Progress and ETA are reported against
    // the whole file, so add back what we already have.
    let total_bytes = res
        .content_length()
        .map(|len| len + if resuming { resume_from } else { 0 });

    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;

    let mut file = if resuming {
        // Seed the hash with the bytes already on disk. The hasher is
        // streaming, so a resumed download that skipped this would finish with
        // a hash of only the tail and fail verification on a perfectly good
        // file.
        let mut existing = File::open(dest_path).await?;
        let mut buf = vec![0u8; 1024 * 1024];
        loop {
            let n = existing.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        drop(existing);
        downloaded = resume_from;
        tracing::info!(resume_from, "resuming model download");
        fs::OpenOptions::new().append(true).open(dest_path).await?
    } else {
        File::create(dest_path).await?
    };

    let mut last_emitted: u64 = downloaded;
    let mut last_emit_instant = Instant::now();
    let mut ema_rate: f64 = 0.0;
    let mut cancelled = false;

    while let Some(item) = res.chunk().await? {
        if cancel_token.as_ref().is_some_and(|t| t.is_cancelled()) {
            cancelled = true;
            break;
        }

        file.write_all(&item).await?;
        hasher.update(&item);
        downloaded += item.len() as u64;

        if let Some((app, model_id)) = progress {
            if downloaded - last_emitted >= PROGRESS_EMIT_INTERVAL_BYTES {
                let now = Instant::now();
                let elapsed = now.duration_since(last_emit_instant).as_secs_f64();
                if elapsed > 0.0 {
                    let instantaneous = (downloaded - last_emitted) as f64 / elapsed;
                    ema_rate = if ema_rate == 0.0 {
                        instantaneous
                    } else {
                        EMA_ALPHA * instantaneous + (1.0 - EMA_ALPHA) * ema_rate
                    };
                }
                last_emitted = downloaded;
                last_emit_instant = now;

                let _ = app.emit(
                    "llm_download_progress",
                    LlmDownloadProgress {
                        model_id: model_id.to_string(),
                        bytes_downloaded: downloaded,
                        total_bytes,
                        bytes_per_sec: ema_rate,
                    },
                );
            }
        }
    }

    if cancelled {
        drop(file);
        fs::remove_file(dest_path).await.ok();
        fs::remove_file(verified_marker_path(dest_path)).await.ok();
        return Ok(());
    }

    file.flush().await?;

    if let Some((app, model_id)) = progress {
        let _ = app.emit(
            "llm_download_progress",
            LlmDownloadProgress {
                model_id: model_id.to_string(),
                bytes_downloaded: downloaded,
                total_bytes,
                bytes_per_sec: ema_rate,
            },
        );
    }

    // Validate hash if expected_hash is a real (non-empty, non-placeholder) value.
    if !expected_hash.is_empty() && !expected_hash.starts_with("PLACEHOLDER_") {
        let actual_hash = format!("{:x}", hasher.finalize());
        if actual_hash != expected_hash {
            fs::remove_file(dest_path).await.ok();
            return Err(anyhow::anyhow!(
                "SHA-256 validation failed. Expected: {}, got: {}",
                expected_hash,
                actual_hash
            ));
        }
        // Marks this exact file as hash-verified so `get_model_path` can
        // trust it on a cheap existence check instead of re-hashing a
        // multi-GB file on every startup. A download interrupted by a
        // crash/network drop never reaches this line, so a corrupt partial
        // file is never marked verified even though it's left on disk.
        fs::write(verified_marker_path(dest_path), &actual_hash)
            .await
            .ok();
    }

    Ok(())
}

fn verified_marker_path(dest_path: &Path) -> PathBuf {
    let mut marker = dest_path.as_os_str().to_owned();
    marker.push(".verified");
    PathBuf::from(marker)
}

/// Per-model-id cancellation tokens for in-progress downloads. A download
/// registers its token at start and unregisters it when it settles
/// (success, error, or cancel alike) — a stale entry would otherwise cancel
/// a *later* unrelated download of the same model id.
#[derive(Default)]
pub struct DownloadRegistry(std::sync::Mutex<std::collections::HashMap<String, CancellationToken>>);

impl DownloadRegistry {
    pub fn register(&self, model_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.0
            .lock()
            .unwrap()
            .insert(model_id.to_string(), token.clone());
        token
    }

    /// No-op if `model_id` isn't currently downloading (already finished,
    /// or never started) — nothing for the caller to distinguish.
    pub fn cancel(&self, model_id: &str) {
        if let Some(token) = self.0.lock().unwrap().get(model_id) {
            token.cancel();
        }
    }

    pub fn unregister(&self, model_id: &str) {
        self.0.lock().unwrap().remove(model_id);
    }
}

/// A model file that's just present on disk isn't necessarily usable --
/// an interrupted/crashed download can leave a truncated `.gguf` that
/// `llama-server` will only discover is broken once it's already spawned
/// (`tensor data is not within the file bounds`), crashing Layer 6
/// extraction for every email until someone notices. `download_file_with_hash`
/// writes a `.verified` marker once a download's SHA-256 has actually been
/// checked, so a marker's presence is trusted outright. Its absence doesn't
/// necessarily mean corruption though -- installs from before this marker
/// existed are real and shouldn't be forced through a redundant multi-GB
/// redownload, so those fall back to a cheap size sanity check: a truncated
/// download is drastically undersized (this app's real-world case was 302MB
/// against an ~9GB model), not off by a few percent.
pub fn get_model_path(app_dir: &Path, model_id: &str) -> Option<PathBuf> {
    let path = app_dir.join("models").join(format!("{}.gguf", model_id));
    if !path.exists() {
        return None;
    }
    if verified_marker_path(&path).exists() {
        return Some(path);
    }

    const MIN_FRACTION_OF_EXPECTED_SIZE: f64 = 0.5;
    let expected_bytes = get_available_models()
        .into_iter()
        .find(|m| m.id == model_id)
        .map(|m| m.approx_size_gb * 1_000_000_000.0);
    let Some(expected_bytes) = expected_bytes else {
        // Unknown model id (not in the current catalog) -- nothing to
        // compare against, fall back to trusting existence as before.
        return Some(path);
    };

    match std::fs::metadata(&path) {
        Ok(meta) if (meta.len() as f64) >= expected_bytes * MIN_FRACTION_OF_EXPECTED_SIZE => {
            Some(path)
        }
        Ok(meta) => {
            tracing::warn!(
                model_id,
                actual_bytes = meta.len(),
                expected_bytes = expected_bytes as u64,
                "Model file is undersized (possibly actively downloading or a truncated/cancelled download)"
            );
            None
        }
        Err(_) => None,
    }
}

/// Default active model when `local_profile.llm_model` is unset — matches
/// the frontend's own `localStorage.getItem('llm_model') || 'gemma4_12b'`
/// default (`src/pages/Settings.tsx`), so an unconfigured backend and an
/// unconfigured frontend agree on the same model without either side having
/// to special-case "nothing chosen yet."
pub const DEFAULT_ACTIVE_MODEL_ID: &str = "gemma4_12b";

/// Resolves which model id should be considered "active" given what's
/// actually downloaded. Never returns an id that isn't in `downloaded`.
/// Order: keep the stored choice if it's still downloaded -> else the
/// catalog default if it's downloaded -> else the lowest-tier downloaded
/// model -> else `None` if nothing is downloaded at all.
pub fn resolve_active_model(downloaded: &[String], stored: Option<&str>) -> Option<String> {
    if let Some(id) = stored {
        if downloaded.iter().any(|d| d == id) {
            return Some(id.to_string());
        }
    }
    if downloaded.iter().any(|d| d == DEFAULT_ACTIVE_MODEL_ID) {
        return Some(DEFAULT_ACTIVE_MODEL_ID.to_string());
    }
    get_available_models()
        .into_iter()
        .filter(|m| downloaded.contains(&m.id))
        .min_by_key(|m| m.tier)
        .map(|m| m.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_ids_and_filenames_agree_with_ladder_layer() {
        // Regression test for the exact bug this fix addresses: ladder.rs's
        // Layer6LlmLayer must resolve to a real catalog id, never a
        // hardcoded string that silently drifts from this list.
        let models = get_available_models();
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&DEFAULT_ACTIVE_MODEL_ID));
    }

    #[test]
    fn every_catalog_entry_has_a_real_non_placeholder_artifact() {
        for model in get_available_models() {
            assert!(
                !model.gguf_url.starts_with("PLACEHOLDER_"),
                "{} still has a placeholder gguf_url",
                model.id
            );
            assert_eq!(
                model.expected_sha256.len(),
                64,
                "{} expected_sha256 is not a 64-char hex sha256",
                model.id
            );
            assert!(
                model.expected_sha256.chars().all(|c| c.is_ascii_hexdigit()),
                "{} expected_sha256 contains non-hex characters",
                model.id
            );
        }
    }

    fn temp_models_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dinero_llm_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("models")).unwrap();
        dir
    }

    /// Regression test for the field bug this fix addresses: a truncated
    /// download (real-world case was 302MB against an ~9GB expected model)
    /// left on disk with no `.verified` marker must not be handed to
    /// `llama-server` as if it were usable.
    #[test]
    fn get_model_path_rejects_drastically_undersized_unverified_file() {
        let app_dir = temp_models_dir();
        let model_path = app_dir.join("models").join("gemma4_12b.gguf");
        std::fs::write(&model_path, vec![0u8; 1024]).unwrap(); // far below ~9GB, no .verified marker

        assert!(get_model_path(&app_dir, "gemma4_12b").is_none());
    }

    /// A `.verified` marker (written only after a real SHA-256 match --
    /// see `download_file_with_hash`) must be trusted outright, even for a
    /// file too small to pass the size heuristic on its own -- the marker
    /// is stronger evidence than a size guess.
    #[test]
    fn get_model_path_trusts_verified_marker_regardless_of_size() {
        let app_dir = temp_models_dir();
        let model_path = app_dir.join("models").join("gemma4_12b.gguf");
        std::fs::write(&model_path, vec![0u8; 1024]).unwrap();
        std::fs::write(verified_marker_path(&model_path), "somehash").unwrap();

        assert_eq!(get_model_path(&app_dir, "gemma4_12b"), Some(model_path));
    }

    /// Installs from before the `.verified` marker existed are real,
    /// correctly-downloaded models and must not be forced through a
    /// redundant multi-GB redownload just because the marker is missing.
    #[test]
    fn get_model_path_accepts_correctly_sized_unmarked_legacy_file() {
        let app_dir = temp_models_dir();
        let model_path = app_dir.join("models").join("gemma4_12b.gguf");
        let approx_bytes = (get_available_models()
            .into_iter()
            .find(|m| m.id == "gemma4_12b")
            .unwrap()
            .approx_size_gb
            * 1_000_000_000.0) as u64;
        // Sparse file via `set_len` -- a real multi-GB write here would make
        // this test itself slow and disk-hungry; only the reported length
        // matters to the size-heuristic under test, not real file content.
        let file = std::fs::File::create(&model_path).unwrap();
        file.set_len(approx_bytes).unwrap();

        assert_eq!(get_model_path(&app_dir, "gemma4_12b"), Some(model_path));
    }

    #[test]
    fn resolve_active_model_keeps_stored_when_still_downloaded() {
        let downloaded = vec!["gemma4_e4b".to_string(), "gemma4_12b".to_string()];
        assert_eq!(
            resolve_active_model(&downloaded, Some("gemma4_e4b")),
            Some("gemma4_e4b".to_string())
        );
    }

    #[test]
    fn resolve_active_model_falls_back_to_default_when_stored_is_gone() {
        let downloaded = vec!["gemma4_12b".to_string(), "qwen3_6_27b".to_string()];
        // stored id was deleted; default ("gemma4_12b") is still downloaded
        assert_eq!(
            resolve_active_model(&downloaded, Some("gemma4_e4b")),
            Some("gemma4_12b".to_string())
        );
    }

    #[test]
    fn resolve_active_model_falls_back_to_lowest_tier_when_default_not_downloaded() {
        let downloaded = vec!["qwen3_6_27b".to_string(), "gemma4_31b".to_string()];
        // stored id gone, default ("gemma4_12b") not downloaded either ->
        // lowest tier among what's downloaded (qwen3_6_27b is tier 3, gemma4_31b is tier 5)
        assert_eq!(
            resolve_active_model(&downloaded, Some("gemma4_e4b")),
            Some("qwen3_6_27b".to_string())
        );
    }

    #[test]
    fn resolve_active_model_returns_none_when_nothing_downloaded() {
        let downloaded: Vec<String> = vec![];
        assert_eq!(resolve_active_model(&downloaded, Some("gemma4_12b")), None);
        assert_eq!(resolve_active_model(&downloaded, None), None);
    }

    #[test]
    fn resolve_active_model_picks_default_when_nothing_stored_yet() {
        let downloaded = vec!["gemma4_e4b".to_string(), "gemma4_12b".to_string()];
        assert_eq!(
            resolve_active_model(&downloaded, None),
            Some("gemma4_12b".to_string())
        );
    }

    /// audit_06 #8: models are 4-20 GB, so an interrupted download used to
    /// throw away everything and start from zero. Resume is only safe if the
    /// streaming hash is seeded from the bytes already on disk -- otherwise a
    /// resumed download finishes with a hash of just the tail and fails
    /// verification on a perfectly good file, which is worse than not
    /// resuming at all.
    #[tokio::test]
    async fn interrupted_download_resumes_and_still_verifies() {
        let body: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
        let expected_hash = format!("{:x}", Sha256::digest(&body));
        let already_have = 1500usize;

        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/model.gguf")
            .match_header("range", format!("bytes={already_have}-").as_str())
            .with_status(206)
            .with_header(
                "content-range",
                &format!("bytes {}-{}/{}", already_have, body.len() - 1, body.len()),
            )
            .with_body(&body[already_have..])
            .create_async()
            .await;

        let dir = std::env::temp_dir().join(format!("dinero_resume_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("model.gguf");
        // The partial a crash or network drop leaves behind.
        std::fs::write(&dest, &body[..already_have]).unwrap();

        download_file_with_hash(
            &format!("{}/model.gguf", server.url()),
            &dest,
            &expected_hash,
            None,
            None,
        )
        .await
        .expect("a resumed download must verify");

        mock.assert_async().await;
        assert_eq!(
            std::fs::read(&dest).unwrap(),
            body,
            "file must be byte-identical"
        );
        assert!(
            verified_marker_path(&dest).exists(),
            "a verified resume must leave the marker get_model_path trusts"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A server that ignores `Range` answers 200 with the *whole* file.
    /// Appending that to the partial would silently corrupt it, and only the
    /// final hash would notice -- after another multi-GB transfer. The status
    /// code, not the request, decides whether we append or start over.
    #[tokio::test]
    async fn server_ignoring_range_restarts_instead_of_appending() {
        let body: Vec<u8> = (0..2048u32).map(|i| (i % 97) as u8).collect();
        let expected_hash = format!("{:x}", Sha256::digest(&body));

        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/model.gguf")
            .with_status(200)
            .with_body(&body)
            .create_async()
            .await;

        let dir = std::env::temp_dir().join(format!("dinero_norange_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("model.gguf");
        std::fs::write(&dest, &body[..700]).unwrap();

        download_file_with_hash(
            &format!("{}/model.gguf", server.url()),
            &dest,
            &expected_hash,
            None,
            None,
        )
        .await
        .expect("a non-range server must still produce a correct file");

        assert_eq!(
            std::fs::read(&dest).unwrap(),
            body,
            "the partial must have been discarded, not appended to"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
