pub mod args;
pub mod events;
pub mod middleware;
pub mod responses;

use crate::error::AppError;

/// TASK-SETUP-013. A reusable, async-safe panic boundary for IPC command
/// bodies.
///
/// Document 19 §3.4 calls for `std::panic::catch_unwind` wrapping every
/// command. That mechanism only catches *synchronous* panics inside the
/// closure passed to it and does not work across `.await` points — but
/// nearly every command in this codebase is `async fn`. The async-safe
/// equivalent is spawning the future on its own Tokio task and inspecting
/// the resulting `JoinError`: Tokio isolates a panicking task from the
/// rest of the runtime and reports it back as an `Err(JoinError)` whose
/// `is_panic()` is true — exactly analogous to `catch_unwind` for
/// synchronous code, and the correct mechanism for this codebase's actual
/// (fully async) command signatures.
///
/// Logs via `tracing::error!` (already flowing into `app-logs.log`, which
/// the diagnostic bundle export reads — Document 19 §21.1) rather than
/// writing an `audit_log` row directly: that would require threading a DB
/// pool into this generic, DB-agnostic primitive. **Not yet wired into
/// the ~53 existing command handlers** (`commands/mod.rs`,
/// `licensing/commands.rs`, etc.) — retrofitting every already-built
/// command is a wide, invasive change spanning most of Area 8's IPC
/// surface. TASK-API-001 ("IPC Request Validation Middleware") is the
/// natural integration point, since a DB pool is already in scope there
/// to also write the `audit_log` row Document 19 describes.
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
            Err(AppError::Internal(format!("IPC task join error: {join_err}")))
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
            with_panic_boundary(async { Err::<i32, _>(AppError::Validation("bad".into())) })
                .await;
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
