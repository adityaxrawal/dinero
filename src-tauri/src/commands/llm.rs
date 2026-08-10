//! Commands controlling the local LLM: catalogue, downloads, active model.
//!
//! Downloads are multi-gigabyte and cancellable, so they are started here and
//! report progress by event rather than blocking the invoking call.
use crate::llm_manager::{self, LlmModelInfo};
use anyhow::Result;
use tauri::{Manager, State};

#[tauri::command]
/// Lists the model catalogue.
pub async fn llm_get_available_models() -> Result<Vec<LlmModelInfo>, crate::error::AppError> {
    Ok(llm_manager::get_available_models())
}

#[tauri::command]
/// Starts a model download, reporting progress by event.
///
/// Downloads are multi-gigabyte, so this returns immediately rather than blocking
/// the IPC call for the duration.
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

#[tauri::command]
/// Cancels an in-flight download.
///
/// Cancellation matters here because a partially written multi-gigabyte file must
/// not be left behind or mistaken for a usable model.
pub async fn llm_cancel_download(
    registry: State<'_, llm_manager::DownloadRegistry>,
    model_id: String,
) -> Result<(), crate::error::AppError> {
    registry.cancel(&model_id);
    Ok(())
}

/// Model ids actually present on disk.
fn downloaded_model_ids(app_dir: &std::path::Path) -> Vec<String> {
    llm_manager::get_available_models()
        .into_iter()
        .filter(|m| llm_manager::get_model_path(app_dir, &m.id).is_some())
        .map(|m| m.id)
        .collect()
}

/// Persists a setting only when it differs from the stored value.
///
/// Avoids a write on every startup reconciliation, which would touch the database
/// for no reason on each launch.
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
/// Deletes a downloaded model and its stored selection.
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

#[tauri::command]
/// Lists models present on disk.
pub async fn llm_get_downloaded_models(
    app: tauri::AppHandle,
) -> Result<Vec<String>, crate::error::AppError> {
    let app_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));

    Ok(downloaded_model_ids(&app_dir))
}

#[tauri::command]
/// Returns the active model, reconciled against what is downloaded.
///
/// A stored preference naming a deleted model degrades to something available,
/// rather than failing later at inference time.
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
/// Sets the active model.
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

#[tauri::command]
/// Reports RAM and core count with the derived model recommendation.
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

#[tauri::command]
/// Sets the parallel slot count, bounded by the safe ceiling.
///
/// The ceiling is memory-derived: exceeding it does not merely slow inference, it
/// pushes the machine into swap.
pub async fn llm_set_parallel_slots(
    slots: usize,
    app: tauri::AppHandle,
    pool: tauri::State<'_, deadpool_sqlite::Pool>,
) -> Result<usize, crate::error::AppError> {
    let model_size_gb = active_model_size_gb(&app, pool.inner()).await;
    let hw = crate::startup::read_hardware_info();
    let ceiling = crate::startup::max_safe_slots(hw.total_ram_gb, model_size_gb);

    let clamped = slots.clamp(1, ceiling);
    if clamped < slots {
        tracing::warn!(
            requested = slots,
            granted = clamped,
            ram_gb = hw.total_ram_gb,
            model_size_gb,
            "parallel slot request exceeds what this machine's RAM can hold — clamped"
        );
    }
    crate::llama_sidecar::set_parallel_slots(clamped);
    Ok(clamped)
}

/// Size of the active model in GB, used to compute the slot ceiling.
async fn active_model_size_gb(app: &tauri::AppHandle, pool: &deadpool_sqlite::Pool) -> f64 {
    use tauri::Manager as _;
    let Ok(app_dir) = app.path().app_data_dir() else {
        return 5.0;
    };
    let downloaded = downloaded_model_ids(&app_dir);
    let stored = match pool.get().await {
        Ok(conn) => conn
            .interact(|c| crate::db::local_profile::get_llm_model(c))
            .await
            .ok()
            .and_then(|r| r.ok())
            .flatten(),
        Err(_) => None,
    };
    llm_manager::resolve_active_model(&downloaded, stored.as_deref())
        .and_then(|id| {
            llm_manager::get_available_models()
                .into_iter()
                .find(|m| m.id == id)
        })
        .map(|m| m.approx_size_gb)
        .unwrap_or(5.0)
}
