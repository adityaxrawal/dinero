use crate::llm_manager::{self, LlmModelInfo};
use anyhow::Result;
use tauri::{Manager, State};

#[tauri::command]
pub async fn llm_get_available_models() -> Result<Vec<LlmModelInfo>, crate::error::AppError> {
    Ok(llm_manager::get_available_models())
}

#[tauri::command]
pub async fn llm_download_model(
    app: tauri::AppHandle,
    registry: State<'_, llm_manager::DownloadRegistry>,
    model_id: String,
) -> Result<(), crate::error::AppError> {
    let app_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));

    let models = llm_manager::get_available_models();
    let model_info = models
        .into_iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| crate::error::AppError::Validation("Model not found".to_string()))?;

    let cancel_token = registry.register(&model_id);
    let result = llm_manager::download_model(&app_dir, &model_info, Some(&app), Some(cancel_token))
        .await
        .map_err(|e| crate::error::AppError::Network(e.to_string()));
    registry.unregister(&model_id);
    result
}

/// No-op if `model_id` isn't currently downloading — nothing for the
/// frontend to distinguish from "already finished."
#[tauri::command]
pub async fn llm_cancel_download(
    registry: State<'_, llm_manager::DownloadRegistry>,
    model_id: String,
) -> Result<(), crate::error::AppError> {
    registry.cancel(&model_id);
    Ok(())
}

fn downloaded_model_ids(app_dir: &std::path::Path) -> Vec<String> {
    llm_manager::get_available_models()
        .into_iter()
        .filter(|m| llm_manager::get_model_path(app_dir, &m.id).is_some())
        .map(|m| m.id)
        .collect()
}

/// Persists `resolved` (the output of `resolve_active_model`) if it differs
/// from `stored`, so a stale DB row self-heals as soon as it's noticed.
async fn persist_if_changed(
    pool: &deadpool_sqlite::Pool,
    stored: Option<String>,
    resolved: Option<String>,
) -> Result<(), crate::error::AppError> {
    if stored == resolved {
        return Ok(());
    }
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    match resolved {
        Some(id) => conn
            .interact(move |c| crate::db::local_profile::set_llm_model(c, &id))
            .await
            .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
            .map_err(|e| crate::error::AppError::Db(e.to_string())),
        None => conn
            .interact(|c| crate::db::local_profile::clear_llm_model(c))
            .await
            .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
            .map_err(|e| crate::error::AppError::Db(e.to_string())),
    }
}

#[tauri::command]
pub async fn llm_delete_model(
    app: tauri::AppHandle,
    pool: State<'_, deadpool_sqlite::Pool>,
    model_id: String,
) -> Result<String, crate::error::AppError> {
    let app_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));

    llm_manager::delete_model(&app_dir, &model_id)
        .map_err(|e| crate::error::AppError::Io(e.to_string()))?;

    let downloaded = downloaded_model_ids(&app_dir);
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let stored = conn
        .interact(|c| crate::db::local_profile::get_llm_model(c))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let resolved = llm_manager::resolve_active_model(&downloaded, stored.as_deref());
    persist_if_changed(&pool, stored, resolved.clone()).await?;

    Ok(resolved.unwrap_or_default())
}

/// Which catalog models already have their `.gguf` file on disk — the
/// Settings model picker previously had no way to show download status at
/// all (or even a way to trigger a download), so every model looked
/// identical regardless of whether Layer 6 could actually use it yet.
#[tauri::command]
pub async fn llm_get_downloaded_models(
    app: tauri::AppHandle,
) -> Result<Vec<String>, crate::error::AppError> {
    let app_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));

    Ok(downloaded_model_ids(&app_dir))
}

/// The model the extraction pipeline's Layer 6 (`Layer6LlmLayer`) will
/// actually try to load — reads `local_profile.llm_model`, self-healing it
/// against what's actually downloaded on disk (a stale row pointing at a
/// deleted model, or an unset row, both resolve to whatever's really
/// available via `resolve_active_model`).
#[tauri::command]
pub async fn llm_get_active_model(
    app: tauri::AppHandle,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<String, crate::error::AppError> {
    let app_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let stored = conn
        .interact(|c| crate::db::local_profile::get_llm_model(c))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;

    let downloaded = downloaded_model_ids(&app_dir);
    let resolved = llm_manager::resolve_active_model(&downloaded, stored.as_deref());
    persist_if_changed(&pool, stored, resolved.clone()).await?;

    Ok(resolved.unwrap_or_default())
}

#[tauri::command]
pub async fn llm_set_active_model(
    pool: State<'_, deadpool_sqlite::Pool>,
    model_id: String,
) -> Result<(), crate::error::AppError> {
    let models = llm_manager::get_available_models();
    if !models.iter().any(|m| m.id == model_id) {
        return Err(crate::error::AppError::Validation(format!(
            "Unknown model id: {model_id}"
        )));
    }

    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    conn.interact(move |c| crate::db::local_profile::set_llm_model(c, &model_id))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))
}

#[derive(serde::Serialize)]
pub struct HardwareRecommendation {
    pub ram_gb: f64,
    pub cpu_cores: usize,
    pub recommended_slots: usize,
    pub recommended_model_id: Option<String>,
}

/// Reads machine RAM/CPU via `sysinfo` and returns a recommended parallel-
/// slot count (sized against the *currently active* model's real weight
/// size, not the recommended tier's — a user already running a heavier
/// model than recommended should still get a slot count that respects that
/// model's actual RAM footprint) plus a recommended model tier for the
/// Settings model picker's badge.
#[tauri::command]
pub async fn llm_get_hardware_info(
    app: tauri::AppHandle,
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<HardwareRecommendation, crate::error::AppError> {
    let app_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));

    let downloaded = downloaded_model_ids(&app_dir);
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let stored = conn
        .interact(|c| crate::db::local_profile::get_llm_model(c))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let active_model_id = llm_manager::resolve_active_model(&downloaded, stored.as_deref());

    let hw = crate::startup::read_hardware_info();
    let model_size_gb = active_model_id
        .as_deref()
        .and_then(|id| {
            llm_manager::get_available_models()
                .into_iter()
                .find(|m| m.id == id)
        })
        .map(|m| m.approx_size_gb)
        .unwrap_or(5.0);

    Ok(HardwareRecommendation {
        ram_gb: hw.total_ram_gb,
        cpu_cores: hw.cpu_cores,
        recommended_slots: crate::startup::compute_recommended_slots(
            hw.total_ram_gb,
            hw.cpu_cores,
            model_size_gb,
        ),
        recommended_model_id: crate::startup::recommend_model_id(hw.total_ram_gb),
    })
}

/// Clamped 1-10 both here and in the frontend's own input — defense in
/// depth, not redundancy: this command is the one place a bad value could
/// actually reach `llama-server`'s `--parallel` flag.
#[tauri::command]
pub fn llm_set_parallel_slots(slots: usize) -> Result<usize, crate::error::AppError> {
    let clamped = slots.clamp(1, 10);
    crate::llama_sidecar::set_parallel_slots(clamped);
    Ok(clamped)
}
