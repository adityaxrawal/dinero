//! Local LLM model catalogue: download, deletion, and active-model selection.
//!
//! Models are large single-file GGUF downloads pulled once from Hugging Face and
//! stored under the app data directory. Downloads are cancellable and tracked in
//! a registry, since a partially written multi-gigabyte file must not be left
//! behind or mistaken for a usable model.
//!
//! Selecting the active model reconciles what the user asked for against what is
//! actually present on disk, so a stored preference naming a deleted model
//! degrades to something available rather than failing at inference time.

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

#[derive(Debug, Clone, Serialize)]
pub struct LlmDownloadProgress {
    pub model_id: String,
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
    pub bytes_per_sec: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmModelInfo {
    pub id: String,
    pub name: String,
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
    pub status: String,
}

/// The model catalogue with its RAM requirements.
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

/// Downloads a model, verifying its hash on completion.
pub async fn download_model(
    app_dir: &Path,
    model_info: &LlmModelInfo,
    progress_app: Option<&AppHandle>,
    cancel_token: Option<CancellationToken>,
) -> Result<()> {
    if model_info.gguf_url.starts_with("PLACEHOLDER_") {
        anyhow::bail!(
            "Model '{}' has no real download URL configured yet (placeholder artifact, see llm_manager.rs)",
            model_info.id
        );
    }

    let models_dir = app_dir.join("models");
    fs::create_dir_all(&models_dir).await?;

    let model_path = models_dir.join(format!("{}.gguf", model_info.id));

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

/// Deletes a downloaded model file.
pub fn delete_model(app_dir: &Path, model_id: &str) -> Result<()> {
    let model_path = app_dir.join("models").join(format!("{}.gguf", model_id));
    let marker_path = verified_marker_path(&model_path);
    let _ = std::fs::remove_file(&model_path);
    let _ = std::fs::remove_file(&marker_path);
    Ok(())
}

/// Downloads a file, streaming to disk and verifying its hash.
///
/// Streamed rather than buffered because these are multi-gigabyte files that
/// would not fit comfortably in memory. Cancellable mid-transfer, and the hash is
/// checked before the result is treated as usable.
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

    let resume_from: u64 = match fs::metadata(dest_path).await {
        Ok(meta) if meta.len() > 0 => meta.len(),
        _ => 0,
    };

    let mut request = client.get(url);
    if resume_from > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={resume_from}-"));
    }
    let mut res = request.send().await?.error_for_status()?;

    let resuming = resume_from > 0 && res.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    if resume_from > 0 && !resuming {
        tracing::info!(
            resume_from,
            status = %res.status(),
            "server did not honour Range — restarting the model download from zero"
        );
    }

    let total_bytes = res
        .content_length()
        .map(|len| len + if resuming { resume_from } else { 0 });

    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;

    let mut file = if resuming {
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
        fs::write(verified_marker_path(dest_path), &actual_hash)
            .await
            .ok();
    }

    Ok(())
}

/// Path of the marker written once a download is verified.
///
/// Its presence is what distinguishes a complete verified model from a partial
/// file left behind by an interrupted download.
fn verified_marker_path(dest_path: &Path) -> PathBuf {
    let mut marker = dest_path.as_os_str().to_owned();
    marker.push(".verified");
    PathBuf::from(marker)
}

#[derive(Default)]
pub struct DownloadRegistry(std::sync::Mutex<std::collections::HashMap<String, CancellationToken>>);

impl DownloadRegistry {
    /// Registers a cancellation token for an in-flight download.
    pub fn register(&self, model_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.0
            .lock()
            .unwrap()
            .insert(model_id.to_string(), token.clone());
        token
    }

    /// Cancels a download in progress.
    pub fn cancel(&self, model_id: &str) {
        if let Some(token) = self.0.lock().unwrap().get(model_id) {
            token.cancel();
        }
    }

    /// Removes a download's token once it has finished.
    pub fn unregister(&self, model_id: &str) {
        self.0.lock().unwrap().remove(model_id);
    }
}

/// Path of a downloaded model, if present and verified.
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

pub const DEFAULT_ACTIVE_MODEL_ID: &str = "gemma4_12b";

/// Reconciles the stored model preference against what is on disk.
///
/// A preference naming a deleted model degrades to something available, rather
/// than failing later when inference is attempted.
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

    #[test]
    fn get_model_path_rejects_drastically_undersized_unverified_file() {
        let app_dir = temp_models_dir();
        let model_path = app_dir.join("models").join("gemma4_12b.gguf");
        std::fs::write(&model_path, vec![0u8; 1024]).unwrap();

        assert!(get_model_path(&app_dir, "gemma4_12b").is_none());
    }

    #[test]
    fn get_model_path_trusts_verified_marker_regardless_of_size() {
        let app_dir = temp_models_dir();
        let model_path = app_dir.join("models").join("gemma4_12b.gguf");
        std::fs::write(&model_path, vec![0u8; 1024]).unwrap();
        std::fs::write(verified_marker_path(&model_path), "somehash").unwrap();

        assert_eq!(get_model_path(&app_dir, "gemma4_12b"), Some(model_path));
    }

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
        assert_eq!(
            resolve_active_model(&downloaded, Some("gemma4_e4b")),
            Some("gemma4_12b".to_string())
        );
    }

    #[test]
    fn resolve_active_model_falls_back_to_lowest_tier_when_default_not_downloaded() {
        let downloaded = vec!["qwen3_6_27b".to_string(), "gemma4_31b".to_string()];
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
