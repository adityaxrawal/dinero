//! In-memory-only holding area for raw PDF bytes blocked on user action:
//! Statement Instrument Gate confirmation (C2 fix, Doc 12 §7.2a) or PDF
//! password entry (H3 fix, Doc 19 §9.4).
//!
//! Entries are never written to disk — dropped on resolution via `take`, or
//! pruned after `ENTRY_TTL` elapses — preserving the "PDF bytes never touch
//! disk" invariant (Doc 12 §7.6.5 / C22).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const ENTRY_TTL: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Default)]
pub struct PendingStatementBytes(Arc<Mutex<HashMap<String, (Vec<u8>, Option<String>, Instant)>>>);

impl PendingStatementBytes {
    /// Holds `bytes` and optional `password` under `statement_id` until `take`n or `ENTRY_TTL` elapses.
    /// Also opportunistically prunes expired entries so memory doesn't grow
    /// unbounded if a user never returns to resolve a blocked statement.
    pub async fn insert(&self, statement_id: String, bytes: Vec<u8>, password: Option<String>) {
        let mut map = self.0.lock().await;
        map.retain(|_, (_, _, inserted)| inserted.elapsed() < ENTRY_TTL);
        map.insert(statement_id, (bytes, password, Instant::now()));
    }

    /// Removes and returns the bytes and optional password for `statement_id`, if still pending and unexpired.
    pub async fn take(&self, statement_id: &str) -> Option<(Vec<u8>, Option<String>)> {
        let mut map = self.0.lock().await;
        match map.remove(statement_id) {
            Some((bytes, password, inserted)) if inserted.elapsed() < ENTRY_TTL => Some((bytes, password)),
            _ => None,
        }
    }

    /// H3 fix: clones the bytes and password for `statement_id` without removing them —
    /// used for password retries, where a wrong attempt must not discard the
    /// bytes needed for the next attempt. Callers that are done with an entry
    /// (success, permanent failure, or timeout) should still use `take`.
    pub async fn peek(&self, statement_id: &str) -> Option<(Vec<u8>, Option<String>)> {
        let map = self.0.lock().await;
        match map.get(statement_id) {
            Some((bytes, password, inserted)) if inserted.elapsed() < ENTRY_TTL => Some((bytes.clone(), password.clone())),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn insert_then_take_returns_bytes_once() {
        let store = PendingStatementBytes::default();
        store.insert("s1".into(), vec![1, 2, 3], None).await;
        assert_eq!(store.take("s1").await, Some((vec![1, 2, 3], None)));
        assert_eq!(store.take("s1").await, None);
    }

    #[tokio::test]
    async fn take_unknown_id_returns_none() {
        let store = PendingStatementBytes::default();
        assert_eq!(store.take("missing").await, None);
    }

    #[tokio::test]
    async fn peek_does_not_remove_entry() {
        let store = PendingStatementBytes::default();
        store.insert("s1".into(), vec![9, 9, 9], Some("pwd".to_string())).await;
        assert_eq!(store.peek("s1").await, Some((vec![9, 9, 9], Some("pwd".to_string()))));
        // Still there after peek — unlike take.
        assert_eq!(store.peek("s1").await, Some((vec![9, 9, 9], Some("pwd".to_string()))));
        assert_eq!(store.take("s1").await, Some((vec![9, 9, 9], Some("pwd".to_string()))));
        assert_eq!(store.peek("s1").await, None);
    }
}
