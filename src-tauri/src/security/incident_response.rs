//! TASK-AUTH-014: Implement Incident Response — Suspicious Activity Detection.
//!
//! Monitors the trigger events documented in Document 22 §19.5/Document 26
//! §"Local Security Incident Response": repeated OAuth failures, repeated DB
//! decryption failures, `PRAGMA integrity_check` failures, audit-log write
//! failure. Response actions are all on-device: revoke the current session
//! (TASK-AUTH-005 logout), disable the affected integration (Gmail disconnect
//! via TASK-AUTH-006, for OAuth-related triggers specifically — a DB
//! integrity failure has nothing to do with Gmail and must not disconnect
//! it), and write an audit trail of the incident itself.
//!
//! Counters are in-memory only (Tauri-managed state), not persisted —
//! surviving only for the current run. A security monitor watching for *this
//! process's* database corruption/decryption failures has no reason to trust
//! a persisted counter it would need to read from the very store it's
//! watching for corruption.
//!
//! **Not wired into every trigger this task names:** "unexpected refresh
//! spikes" and "audit-log write failure" detection are not implemented here
//! — the former has no existing rate-tracking to hook into without a larger
//! addition to `ingestion::polling`, and the latter is closer to
//! unimplementable-by-construction than merely unwired: if `audit_log::insert`
//! itself is failing, recording *that* failure by calling `audit_log::insert`
//! again is circular. Wired: repeated DB decryption failures (invalid-token
//! Keychain reads are the closest analogous "credential looks compromised"
//! signal already surfaced by TASK-AUTH-004's degradation path) and
//! `PRAGMA integrity_check` failures (TASK-DB-019).

use anyhow::Result;
use deadpool_sqlite::Pool;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

/// The trigger conditions Document 22 §19.5 names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TriggerKind {
    RepeatedOAuthFailure,
    RepeatedDbDecryptionFailure,
    IntegrityCheckFailure,
}

impl TriggerKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::RepeatedOAuthFailure => "repeated_oauth_failure",
            Self::RepeatedDbDecryptionFailure => "repeated_db_decryption_failure",
            Self::IntegrityCheckFailure => "integrity_check_failure",
        }
    }

    /// How many occurrences before this trigger fires a response. Document
    /// 22/26 name the trigger *conditions* but not a specific count for
    /// "repeated" — 3 is an engineering default (same class of judgment call
    /// as Document 18 §4.21b's retention-window default), not sourced from
    /// any document; adjust if a specific count is ever specified.
    fn threshold(&self) -> u32 {
        match self {
            Self::IntegrityCheckFailure => 1, // a single corrupt DB is already an incident
            _ => 3,
        }
    }
}

/// Tauri-managed in-memory counters, one per `TriggerKind`. Register via
/// `app.manage(...)`.
#[derive(Default)]
pub struct IncidentMonitor(Mutex<std::collections::HashMap<TriggerKind, u32>>);

/// Records one occurrence of `kind`. Returns `true` exactly on the tick that
/// crosses the threshold (the counter resets immediately after firing, so a
/// sustained failure condition triggers a response once per fresh run of
/// failures, not on every single one).
pub fn record_trigger(monitor: &IncidentMonitor, kind: TriggerKind) -> bool {
    let mut counters = monitor.0.lock().unwrap();
    let count = counters.entry(kind).or_insert(0);
    *count += 1;
    if *count >= kind.threshold() {
        *count = 0;
        true
    } else {
        false
    }
}

/// Executes the on-device incident response for `kind`: writes an
/// `audit_log` entry describing the incident, emits a `security_incident`
/// event (for the frontend to prompt re-authentication), and revokes the
/// current session — always, regardless of trigger, since any of these
/// conditions is reason enough to require re-auth. For OAuth-specific
/// triggers, additionally disconnects every connected Gmail account (Document
/// 22 §19.5: "disable the affected integration ... if token theft
/// suspected").
pub async fn respond_to_incident<R: tauri::Runtime>(
    kind: TriggerKind,
    app: &AppHandle<R>,
    pool: &Pool,
    session_state: &crate::auth::session::SessionState,
) -> Result<()> {
    tracing::warn!("Security incident detected: {}", kind.as_str());

    if let Ok(conn) = pool.get().await {
        let kind_str = kind.as_str();
        let _ = conn
            .interact(move |c| {
                crate::db::audit_log::insert(
                    c,
                    &crate::db::audit_log::AuditLogRow {
                        id: uuid::Uuid::new_v4().to_string(),
                        actor_type: Some("system".to_string()),
                        actor_id: None,
                        action: Some("security_incident_detected".to_string()),
                        resource_type: Some("security".to_string()),
                        resource_id: None,
                        before_json: None,
                        after_json: Some(serde_json::json!({ "trigger": kind_str })),
                        created_at: chrono::Utc::now(),
                    },
                )
            })
            .await;
    }

    let _ = app.emit(
        "security_incident",
        serde_json::json!({ "trigger": kind.as_str() }),
    );

    if kind == TriggerKind::RepeatedOAuthFailure {
        crate::ingestion::oauth::revoke_gmail_access(pool).await;
    }

    crate::auth::session::logout(pool, session_state).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_trigger_fires_exactly_at_threshold_then_resets() {
        let monitor = IncidentMonitor::default();
        assert!(!record_trigger(&monitor, TriggerKind::RepeatedOAuthFailure));
        assert!(!record_trigger(&monitor, TriggerKind::RepeatedOAuthFailure));
        assert!(record_trigger(&monitor, TriggerKind::RepeatedOAuthFailure));

        // Counter reset after firing — takes another full run to fire again.
        assert!(!record_trigger(&monitor, TriggerKind::RepeatedOAuthFailure));
    }

    #[test]
    fn integrity_check_failure_fires_on_the_first_occurrence() {
        let monitor = IncidentMonitor::default();
        assert!(record_trigger(&monitor, TriggerKind::IntegrityCheckFailure));
    }

    #[test]
    fn different_trigger_kinds_have_independent_counters() {
        let monitor = IncidentMonitor::default();
        assert!(!record_trigger(&monitor, TriggerKind::RepeatedOAuthFailure));
        assert!(!record_trigger(&monitor, TriggerKind::RepeatedDbDecryptionFailure));
        assert!(!record_trigger(&monitor, TriggerKind::RepeatedDbDecryptionFailure));
        // OAuth failure at 1, DB decryption at 2 (threshold 3 for both) —
        // neither has fired yet, and one more OAuth failure alone must not
        // be enough to cross DB decryption's independent counter.
        assert!(!record_trigger(&monitor, TriggerKind::RepeatedOAuthFailure));
    }
}
