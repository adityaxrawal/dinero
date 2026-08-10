//! Guards applied to commands before they run.
//!
//! `require_active_session` gates commands that must not execute without a live
//! local session, so incident response revoking a session takes effect
//! immediately across every subsequent call.
use crate::auth::session::{current_session_id, SessionState};
use crate::error::AppError;

/// Requires a live session, rejecting the command otherwise.
///
/// What makes session revocation by incident response take effect immediately
/// across every subsequent write.
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
