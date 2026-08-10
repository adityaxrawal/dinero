//! Cross-cutting concerns for the IPC boundary.
//!
//! `with_panic_boundary` is the important one: a panic inside a command would
//! otherwise unwind into the Tauri runtime, where the frontend sees only a dead
//! call with no error. Catching it converts the panic into a structured error the
//! UI can report, and keeps one bad command from destabilising the process.
pub mod args;
pub mod events;
pub mod middleware;
pub mod responses;
pub mod system_warnings;
pub mod validation;

use crate::error::AppError;

/// Runs a command future, converting a panic into a structured error.
///
/// Without this a panic unwinds into the Tauri runtime and the frontend sees a
/// dead call with no error at all.
pub async fn with_panic_boundary<F, T>(fut: F) -> Result<T, AppError>
where
    F: std::future::Future<Output = Result<T, AppError>> + Send + 'static,
    T: Send + 'static,
{
    match tokio::spawn(fut).await {
        Ok(result) => result,
        Err(join_err) if join_err.is_panic() => {
            let panic_payload = join_err.into_panic();
            let message = panic_payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic_payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "IPC command panicked".to_string());
            tracing::error!("IPC command panic caught by panic boundary: {}", message);
            Err(AppError::Internal(message))
        }
        Err(join_err) => {
            tracing::error!("IPC command task join error (not a panic): {}", join_err);
            Err(AppError::Internal(format!(
                "IPC task join error: {join_err}"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ok_future_passes_through_unchanged() {
        let result = with_panic_boundary(async { Ok::<_, AppError>(42) }).await;
        assert!(matches!(result, Ok(42)));
    }

    #[tokio::test]
    async fn err_future_passes_through_unchanged() {
        let result =
            with_panic_boundary(async { Err::<i32, _>(AppError::Validation("bad".into())) }).await;
        assert!(matches!(result, Err(AppError::Validation(m)) if m == "bad"));
    }

    #[tokio::test]
    async fn panic_is_caught_and_mapped_to_internal_error() {
        let result: Result<i32, AppError> = with_panic_boundary(async {
            panic!("boom");
            #[allow(unreachable_code)]
            Ok(0)
        })
        .await;
        match result {
            Err(AppError::Internal(msg)) => assert!(msg.contains("boom")),
            other => panic!("expected AppError::Internal containing 'boom', got {other:?}"),
        }
    }
}
