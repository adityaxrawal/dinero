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

    llm_manager::download_model(&app_dir, &model_info, Some(&app))
        .await
        .map_err(|e| crate::error::AppError::Network(e.to_string()))
}

#[tauri::command]
pub async fn llm_delete_model(
    app: tauri::AppHandle,
    model_id: String,
) -> Result<(), crate::error::AppError> {
    let app_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));

    llm_manager::delete_model(&app_dir, &model_id)
        .map_err(|e| crate::error::AppError::Io(e.to_string()))
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

    Ok(llm_manager::get_available_models()
        .into_iter()
        .filter(|m| llm_manager::get_model_path(&app_dir, &m.id).is_some())
        .map(|m| m.id)
        .collect())
}

/// The model the extraction pipeline's Layer 6 (`Layer6LlmLayer`) will
/// actually try to load — reads `local_profile.llm_model`, the same column
/// onboarding already writes via `onboarding_save_preferences` but that
/// nothing, anywhere, ever read back until now.
#[tauri::command]
pub async fn llm_get_active_model(
    pool: State<'_, deadpool_sqlite::Pool>,
) -> Result<String, crate::error::AppError> {
    let conn = pool
        .get()
        .await
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    let model_id = conn
        .interact(|c| crate::db::local_profile::get_llm_model(c))
        .await
        .map_err(|e| crate::error::AppError::Unknown(e.to_string()))?
        .map_err(|e| crate::error::AppError::Db(e.to_string()))?;
    Ok(model_id.unwrap_or_else(|| llm_manager::DEFAULT_ACTIVE_MODEL_ID.to_string()))
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
