//! TASK-OPS-007: Log Rotation, Retention, and Redaction Policies.
//!
//! Consolidates what was previously split between `lib.rs` (rotation +
//! retention, private free functions with no test coverage) and
//! `diagnostics.rs` (`redact()`, applied only lazily at bundle-export time)
//! into one testable module. The key behavioral change this task makes:
//! `redact()` is now also applied at **write time** via `RedactingWriter`,
//! wrapping the file appender directly — previously `app-logs.log` itself
//! held unredacted content indefinitely on disk between the (rare) moments
//! someone actually exported a diagnostic bundle, which is the only place
//! redaction used to happen. The console/stdout layer (`lib.rs`'s dev-visible
//! layer) is deliberately left unredacted, matching existing local-dev
//! ergonomics — only the on-disk file, which persists and could be pulled
//! off the machine by any means other than the export flow, is redacted.

use regex::Regex;
use std::io::Write;
use std::path::Path;

/// Doc 28 §4.2 (J4 fix): default retention for rotated `app-logs.log.*`
/// files, overridable via `DINERO_LOG_RETENTION_DAYS` (the doc calls the
/// window "configurable").
pub const DEFAULT_LOG_RETENTION_DAYS: u64 = 15;

/// Deletes rotated log files older than the retention window. Best-effort —
/// a failure here should never block startup.
pub fn prune_old_logs(log_dir: &Path) {
    let retention_days = std::env::var("DINERO_LOG_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_LOG_RETENTION_DAYS);
    let max_age = std::time::Duration::from_secs(retention_days * 24 * 60 * 60);

    let entries = match std::fs::read_dir(log_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("Failed to read log directory for pruning: {}", e);
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let is_rotated_log = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("app-logs.log"))
            .unwrap_or(false);
        if !is_rotated_log {
            continue;
        }

        let age = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|modified| modified.elapsed().ok());

        if let Some(age) = age {
            if age > max_age {
                if let Err(e) = std::fs::remove_file(&path) {
                    tracing::warn!("Failed to prune old log file {:?}: {}", path, e);
                } else {
                    tracing::info!(
                        "Pruned log file older than {} days: {:?}",
                        retention_days,
                        path
                    );
                }
            }
        }
    }
}

/// Doc 19 §21.1, Doc 36 §4 (moved from `diagnostics.rs`, unchanged): masks
/// the highest-risk patterns that could appear in any free-text log line —
/// email, 16-digit card number, ₹-amount, bearer/token/password/secret
/// key-value pairs, and any other 6+-digit run (account numbers, phone
/// numbers, OTPs). Not a formal PII-scrubbing guarantee, but the aggressive
/// whitelist principle (only ERROR-level lines and panic reports ever reach
/// a diagnostic bundle in the first place) plus this pass together are the
/// two-layer defense this task's `test_sensitive_fields_are_never_logged`
/// exercises.
pub fn redact(text: &str) -> String {
    let email_re = Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}").unwrap();
    let card_re = Regex::new(r"\b\d{4}[\s-]?\d{4}[\s-]?\d{4}[\s-]?\d{4}\b").unwrap();
    let rupee_amount_re = Regex::new(r"₹\s?[\d,]+(\.\d{1,2})?").unwrap();
    let digits_re = Regex::new(r"\d{6,}").unwrap();
    let bearer_re = Regex::new(r"(?i)(bearer|token|password|secret)\s*[:=]\s*\S+").unwrap();

    let text = email_re.replace_all(text, "[REDACTED_EMAIL]");
    let text = card_re.replace_all(&text, "[REDACTED_CARD]");
    let text = rupee_amount_re.replace_all(&text, "[REDACTED_AMOUNT]");
    let text = digits_re.replace_all(&text, "[REDACTED_NUMBER]");
    let text = bearer_re.replace_all(&text, "$1: [REDACTED]");
    text.into_owned()
}

/// Wraps any `Write` (here, the `tracing_appender` non-blocking file writer)
/// so every write is passed through `redact()` first. Non-UTF8 chunks are
/// written through unredacted rather than dropped — `tracing`'s own fmt
/// layer only ever writes valid UTF-8 formatted log lines in practice, and
/// silently dropping bytes would be a worse failure mode than an
/// unredacted-but-intact line in the rare case this assumption is wrong.
pub struct RedactingWriter<W: Write> {
    inner: W,
}

impl<W: Write> RedactingWriter<W> {
    pub fn new(inner: W) -> Self {
        Self { inner }
    }
}

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match std::str::from_utf8(buf) {
            Ok(s) => {
                let redacted = redact(s);
                self.inner.write_all(redacted.as_bytes())?;
                Ok(buf.len())
            }
            Err(_) => self.inner.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_catches_email_card_and_rupee_amount() {
        let input =
            "Contact user@example.com about card 4111 1111 1111 1111 for the ₹49,999.50 charge";
        let redacted = redact(input);
        assert!(!redacted.contains("user@example.com"));
        assert!(!redacted.contains("4111 1111 1111 1111"));
        assert!(!redacted.contains("₹49,999.50"));
        assert!(redacted.contains("[REDACTED_EMAIL]"));
        assert!(redacted.contains("[REDACTED_CARD]"));
        assert!(redacted.contains("[REDACTED_AMOUNT]"));
    }

    #[test]
    fn redact_catches_small_amounts_the_old_six_digit_only_regex_missed() {
        let redacted = redact("Groceries: ₹499.00");
        assert!(!redacted.contains("499"));
    }

    /// Doc 30 TASK-OPS-007 acceptance: `test_sensitive_fields_are_never_logged`
    /// (the write-time half). Previously `redact()` only ran at bundle-export
    /// time -- this proves a line written through `RedactingWriter` is
    /// redacted in the *destination* buffer itself, not merely redactable if
    /// someone later chooses to scan it.
    #[test]
    fn redacting_writer_redacts_before_reaching_the_inner_writer() {
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut writer = RedactingWriter::new(&mut buf);
            write!(writer, "user email is realuser@example.com, card 4111-1111-1111-1111").unwrap();
        }
        let written = String::from_utf8(buf).unwrap();
        assert!(!written.contains("realuser@example.com"));
        assert!(!written.contains("4111-1111-1111-1111"));
        assert!(written.contains("[REDACTED_EMAIL]"));
        assert!(written.contains("[REDACTED_CARD]"));
    }

    /// Doc 30 TASK-OPS-007 acceptance: `test_rotation_and_retention_policy_applied`.
    /// Both the default-window and custom-window cases run sequentially in
    /// one test (rather than two separate `#[test]` fns) since both mutate
    /// the same process-wide `DINERO_LOG_RETENTION_DAYS` env var --
    /// `cargo test`'s default parallelism would otherwise race two tests
    /// against the same env var.
    #[test]
    fn prune_old_logs_applies_the_retention_window() {
        let dir = std::env::temp_dir().join(format!("dinero_log_retention_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let old_file = dir.join("app-logs.log.2020-01-01");
        let recent_file = dir.join("app-logs.log.2026-07-22");
        let unrelated_file = dir.join("not-a-log-file.txt");
        std::fs::write(&old_file, "old").unwrap();
        std::fs::write(&recent_file, "recent").unwrap();
        std::fs::write(&unrelated_file, "unrelated").unwrap();

        // Backdate "old" well past any retention window this test
        // configures, and "recent" to 3 days -- old enough to be pruned
        // under a 1-day custom window, but not the 15-day default.
        let far_past = std::time::SystemTime::now() - std::time::Duration::from_secs(400 * 24 * 60 * 60);
        std::fs::File::open(&old_file).unwrap().set_modified(far_past).unwrap();
        let three_days_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 24 * 60 * 60);
        std::fs::File::open(&recent_file).unwrap().set_modified(three_days_ago).unwrap();

        std::env::set_var("DINERO_LOG_RETENTION_DAYS", "15");
        prune_old_logs(&dir);
        std::env::remove_var("DINERO_LOG_RETENTION_DAYS");

        assert!(!old_file.exists(), "a log file older than the retention window must be pruned");
        assert!(recent_file.exists(), "a 3-day-old file must survive the 15-day default window");
        assert!(unrelated_file.exists(), "pruning must never touch a non-log file in the same directory");

        // Now apply a 1-day custom window -- the surviving 3-day-old file
        // must be pruned under this shorter, explicitly configured window.
        std::env::set_var("DINERO_LOG_RETENTION_DAYS", "1");
        prune_old_logs(&dir);
        std::env::remove_var("DINERO_LOG_RETENTION_DAYS");

        assert!(!recent_file.exists(), "a custom shorter retention window must be honored");
        assert!(unrelated_file.exists(), "pruning must still never touch a non-log file");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
