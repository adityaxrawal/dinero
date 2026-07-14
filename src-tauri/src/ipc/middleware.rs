//! TASK-AUTH-008: Enforce Tenant Isolation Pattern Across All IPC Handlers.
//!
//! Document 22 §13.1: Dinero is single-tenant per device — there is no
//! multi-tenant row isolation to enforce (`local_profile.id` can only ever
//! be `1`, TASK-DB-003). What *is* mandatory defense-in-depth is that the
//! "current profile/session" is always resolved from Rust-side
//! `SessionState` (TASK-AUTH-005), never from a React-supplied argument — a
//! forged IPC parameter must have nothing to attach to, structurally, not
//! merely be rejected by an incidental check.

use crate::auth::session::{current_session_id, SessionState};
use crate::error::AppError;

/// Returns the current session id, or an `AppError::Auth` if no active
/// session exists (e.g. after `auth_logout`, before the next launch
/// re-establishes one via `ensure_active_session`). Call this at the top of
/// any Gmail/licensing command that must require re-auth rather than
/// silently operating in a logged-out state.
pub fn require_active_session(state: &SessionState) -> Result<String, AppError> {
    current_session_id(state).ok_or_else(|| AppError::Auth("no_active_session".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errs_with_no_active_session_when_state_is_empty() {
        let state = SessionState::default();
        let result = require_active_session(&state);
        assert!(matches!(result, Err(AppError::Auth(m)) if m == "no_active_session"));
    }

    #[test]
    fn returns_the_session_id_when_one_is_set() {
        let state = SessionState::default();
        *state.0.lock().unwrap() = Some("sess_abc".to_string());
        assert_eq!(require_active_session(&state).unwrap(), "sess_abc");
    }
}
