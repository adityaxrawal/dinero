//! Dedicated Multi-Category Logging System for Dinero.
//!
//! Provides daily rolling, write-time PII redacting, multi-target log files:
//! - `logs/backend.log`: Rust backend execution, DB, ingestion, extraction, lifecycle.
//! - `logs/frontend.log`: React UI, component states, navigation, renderer errors.
//! - `logs/api_calls.log`: Frontend <-> Backend Tauri IPC commands.
//! - `logs/network.log`: Outbound HTTP/HTTPS requests, OAuth, downloads.
//! - `logs/llm_calls.log`: Every LLM sidecar request/response — model, call type, attempt,
//!   prompt/output char counts, wall-clock duration, outcome. Full prompt and output
//!   bodies are gated at TRACE (`RUST_LOG=llm_calls=trace`); metadata always at INFO.
//! - `logs/combined.log`: Master chronological log stream.

pub mod llm_logger;

use regex::Regex;
use std::fs;
use std::io::Write;
use std::path::Path;
use tracing_appender::non_blocking::NonBlocking;
use tracing_subscriber::fmt::writer::MakeWriter;

/// Default retention window for log files (15 days), overridable via `DINERO_LOG_RETENTION_DAYS`.
pub const DEFAULT_LOG_RETENTION_DAYS: u64 = 15;

/// The log categories `CategorizedLogWriters::init` opens one file per, per
/// app launch. Single source of truth for both the writers and
/// `prune_old_logs` -- these drifting apart is exactly what made retention a
/// silent no-op (audit_09 #4). `app-logs` is legacy-only, kept so old installs
/// still get swept.
const LOG_CATEGORY_BASENAMES: [&str; 7] = [
    "backend",
    "frontend",
    "api_calls",
    "network",
    "llm_calls",
    "combined",
    "app-logs",
];

/// Deletes rotated log files older than the retention window.
pub fn prune_old_logs(log_dir: &Path) {
    let retention_days = std::env::var("DINERO_LOG_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_LOG_RETENTION_DAYS);
    let max_age = std::time::Duration::from_secs(retention_days * 24 * 60 * 60);

    let entries = match fs::read_dir(log_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(
                "Failed to read log directory for pruning ({:?}): {}",
                log_dir,
                e
            );
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // audit_09 #4: these patterns used to be `starts_with("backend.log")`
        // and friends -- the *old* naming scheme. `CategorizedLogWriters::init`
        // writes `<IST timestamp>_<category>.md` (e.g.
        // `2026-08-01_09-15-42_backend.md`), so nothing the app actually
        // produces ever matched and pruning was a no-op in production: six new
        // files per launch, never deleted. The unit test passed because it
        // constructed files under the legacy names.
        //
        // Both schemes are matched so existing installs get their old files
        // cleaned up too.
        let is_target_log = LOG_CATEGORY_BASENAMES.iter().any(|base| {
            file_name.ends_with(&format!("_{base}.md"))
                || file_name.starts_with(&format!("{base}.log"))
        });

        if !is_target_log {
            continue;
        }

        let age = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|modified| modified.elapsed().ok());

        if let Some(age) = age {
            if age > max_age {
                if let Err(e) = fs::remove_file(&path) {
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

/// Masks sensitive patterns (emails, 16-digit card numbers, rupee amounts, passwords/tokens, 6+ digit OTPs).
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

/// Writer wrapper that applies write-time PII redaction.
#[derive(Clone)]
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

/// Composite writer writing to both a category-specific log file and the combined log file.
pub struct MultiWrite {
    target_writer: NonBlocking,
    combined_writer: NonBlocking,
}

impl Write for MultiWrite {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = self.target_writer.write_all(buf);
        let _ = self.combined_writer.write_all(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let _ = self.target_writer.flush();
        let _ = self.combined_writer.flush();
        Ok(())
    }
}

/// Categorized log writers for routing tracing events by target name.
#[derive(Clone)]
pub struct CategorizedLogWriters {
    backend: NonBlocking,
    frontend: NonBlocking,
    api_calls: NonBlocking,
    network: NonBlocking,
    llm_calls: NonBlocking,
    combined: NonBlocking,
}

impl CategorizedLogWriters {
    pub fn init(base_log_dir: &Path) -> (Self, Vec<tracing_appender::non_blocking::WorkerGuard>) {
        let logs_folder = base_log_dir.join("logs");
        if let Err(e) = fs::create_dir_all(&logs_folder) {
            eprintln!("Failed to create logs directory {:?}: {}", logs_folder, e);
        }

        prune_old_logs(&logs_folder);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let secs = now.as_secs() + 19800; // IST (UTC+5:30)
        let days = secs / 86400;
        let (y, mo, d) = llm_logger::epoch_days_to_ymd(days);
        let h = (secs / 3600) % 24;
        let m = (secs / 60) % 60;
        let s = secs % 60;
        let ts_prefix = format!("{:04}-{:02}-{:02}_{:02}-{:02}-{:02}", y, mo, d, h, m, s);

        let (backend_w, g1) = tracing_appender::non_blocking(RedactingWriter::new(
            tracing_appender::rolling::never(&logs_folder, format!("{}_backend.md", ts_prefix)),
        ));
        let (frontend_w, g2) = tracing_appender::non_blocking(RedactingWriter::new(
            tracing_appender::rolling::never(&logs_folder, format!("{}_frontend.md", ts_prefix)),
        ));
        let (api_calls_w, g3) = tracing_appender::non_blocking(RedactingWriter::new(
            tracing_appender::rolling::never(&logs_folder, format!("{}_api_calls.md", ts_prefix)),
        ));
        let (network_w, g4) = tracing_appender::non_blocking(RedactingWriter::new(
            tracing_appender::rolling::never(&logs_folder, format!("{}_network.md", ts_prefix)),
        ));
        // Dedicated LLM call log: one entry per sidecar request/response,
        // including model ID, call type, attempt, duration, outcome, and
        // full prompt/output bodies in structured Markdown format.
        let (llm_calls_w, g5) = tracing_appender::non_blocking(RedactingWriter::new(
            tracing_appender::rolling::never(&logs_folder, format!("{}_llm_calls.md", ts_prefix)),
        ));
        let (combined_w, g6) = tracing_appender::non_blocking(RedactingWriter::new(
            tracing_appender::rolling::never(&logs_folder, format!("{}_combined.md", ts_prefix)),
        ));

        let writers = Self {
            backend: backend_w,
            frontend: frontend_w,
            api_calls: api_calls_w,
            network: network_w,
            llm_calls: llm_calls_w,
            combined: combined_w,
        };

        // Seed the direct writer for rich Markdown LLM call blocks
        let llm_log_path = logs_folder.join(format!("{}_llm_calls.md", ts_prefix));
        crate::logging::llm_logger::init_direct_writer(llm_log_path);

        (writers, vec![g1, g2, g3, g4, g5, g6])
    }
}

impl<'a> MakeWriter<'a> for CategorizedLogWriters {
    type Writer = MultiWrite;

    fn make_writer(&'a self) -> Self::Writer {
        MultiWrite {
            target_writer: self.backend.clone(),
            combined_writer: self.combined.clone(),
        }
    }

    fn make_writer_for(&'a self, meta: &tracing::Metadata<'_>) -> Self::Writer {
        let target = meta.target();
        let target_writer = match target {
            "frontend" => self.frontend.clone(),
            "api_calls" => self.api_calls.clone(),
            "network" => self.network.clone(),
            // LLM call logs go to their own file; they also appear in combined.
            "llm_calls" => self.llm_calls.clone(),
            _ => self.backend.clone(),
        };

        MultiWrite {
            target_writer,
            combined_writer: self.combined.clone(),
        }
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
    fn redact_catches_small_amounts() {
        let redacted = redact("Groceries: ₹499.00");
        assert!(!redacted.contains("499"));
    }

    #[test]
    fn redacting_writer_redacts_before_reaching_inner() {
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut writer = RedactingWriter::new(&mut buf);
            write!(
                writer,
                "user email is realuser@example.com, card 4111-1111-1111-1111"
            )
            .unwrap();
        }
        let written = String::from_utf8(buf).unwrap();
        assert!(!written.contains("realuser@example.com"));
        assert!(!written.contains("4111-1111-1111-1111"));
        assert!(written.contains("[REDACTED_EMAIL]"));
        assert!(written.contains("[REDACTED_CARD]"));
    }

    #[test]
    fn prune_old_logs_applies_retention_window() {
        let dir =
            std::env::temp_dir().join(format!("dinero_log_retention_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();

        // audit_09 #4: the names here must be the ones
        // `CategorizedLogWriters::init` actually writes
        // (`<IST timestamp>_<category>.md`). This test previously used only
        // the legacy `backend.log.<date>` scheme, so it kept passing while
        // pruning silently matched nothing in production.
        let old_file = dir.join("2020-01-01_08-00-00_backend.md");
        let recent_file = dir.join("2026-07-22_08-00-00_backend.md");
        // Every other category must be swept too, not just `backend`.
        let old_llm_file = dir.join("2020-01-01_08-00-00_llm_calls.md");
        let old_combined_file = dir.join("2020-01-01_08-00-00_combined.md");
        // Legacy naming still has to be cleaned up on upgraded installs.
        let old_legacy_file = dir.join("backend.log.2020-01-01");
        let unrelated_file = dir.join("not-a-log-file.txt");
        // A Markdown file that is not one of ours must survive.
        let unrelated_md = dir.join("README.md");

        for f in [
            &old_file,
            &recent_file,
            &old_llm_file,
            &old_combined_file,
            &old_legacy_file,
            &unrelated_file,
            &unrelated_md,
        ] {
            fs::write(f, "x").unwrap();
        }

        let far_past =
            std::time::SystemTime::now() - std::time::Duration::from_secs(400 * 24 * 60 * 60);
        for f in [
            &old_file,
            &old_llm_file,
            &old_combined_file,
            &old_legacy_file,
        ] {
            fs::File::open(f).unwrap().set_modified(far_past).unwrap();
        }
        // Age the non-log files too, so surviving proves the name filter is
        // doing the work rather than the age check.
        for f in [&unrelated_file, &unrelated_md] {
            fs::File::open(f).unwrap().set_modified(far_past).unwrap();
        }
        let three_days_ago =
            std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 24 * 60 * 60);
        fs::File::open(&recent_file)
            .unwrap()
            .set_modified(three_days_ago)
            .unwrap();

        std::env::set_var("DINERO_LOG_RETENTION_DAYS", "15");
        prune_old_logs(&dir);
        std::env::remove_var("DINERO_LOG_RETENTION_DAYS");

        assert!(
            !old_file.exists(),
            "a log file older than the retention window must be pruned"
        );
        assert!(
            !old_llm_file.exists(),
            "every log category must be pruned, not just backend"
        );
        assert!(
            !old_combined_file.exists(),
            "every log category must be pruned, not just backend"
        );
        assert!(
            !old_legacy_file.exists(),
            "legacy-named log files must still be pruned on upgraded installs"
        );
        assert!(
            recent_file.exists(),
            "a 3-day-old file must survive the 15-day default window"
        );
        assert!(
            unrelated_file.exists(),
            "pruning must never touch a non-log file in the same directory"
        );
        assert!(
            unrelated_md.exists(),
            "pruning must not delete unrelated .md files"
        );

        std::env::set_var("DINERO_LOG_RETENTION_DAYS", "1");
        prune_old_logs(&dir);
        std::env::remove_var("DINERO_LOG_RETENTION_DAYS");

        assert!(
            !recent_file.exists(),
            "a custom shorter retention window must be honored"
        );
        assert!(
            unrelated_file.exists(),
            "pruning must still never touch a non-log file"
        );
        assert!(
            unrelated_md.exists(),
            "pruning must still not delete unrelated .md files"
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
