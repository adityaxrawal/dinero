use anyhow::{Context, Result};
use chrono::Local;
use regex::Regex;
use rusqlite::Connection;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Doc 19 §21.1, Doc 36 §4: defense-in-depth redaction pass applied to any
/// free-text content (crash reports, error-log lines) before it enters a
/// diagnostic bundle. This is not a formal PII-scrubbing guarantee — the
/// primary safeguard is the aggressive-whitelist principle itself: only
/// ERROR-level log lines and panic reports are considered at all (never the
/// full application log, which can contain arbitrary interpolated values
/// from anywhere in the codebase); this pass additionally masks the two
/// highest-risk patterns that could slip into even those narrower sources.
fn redact(text: &str) -> String {
    let email_re = Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}").unwrap();
    let digits_re = Regex::new(r"\d{6,}").unwrap();
    let bearer_re = Regex::new(r"(?i)(bearer|token|password|secret)\s*[:=]\s*\S+").unwrap();

    let text = email_re.replace_all(text, "[REDACTED_EMAIL]");
    let text = digits_re.replace_all(&text, "[REDACTED_NUMBER]");
    let text = bearer_re.replace_all(&text, "$1: [REDACTED]");
    text.into_owned()
}

/// Doc 19 §21.1's "Bundle includes: ... error traces and panic logs
/// (redacted of PII and financial data)" — only ERROR-level lines from the
/// rolling app log are considered (an aggressive whitelist: most financial
/// data flows through info/debug-level pipeline logs, not error logs), each
/// additionally passed through `redact`.
fn collect_error_lines(log_dir: &Path, max_lines: usize) -> String {
    // J4 fix: the log now rotates daily as `app-logs.log.YYYY-MM-DD` rather
    // than a single fixed `app-logs.log` file, so the most recent rotated
    // file must be found rather than assumed by name.
    let Ok(entries) = std::fs::read_dir(log_dir) else {
        return String::new();
    };
    let latest = entries
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| n.starts_with("app-logs.log"))
                .unwrap_or(false)
        })
        .max_by_key(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });

    let Some(latest) = latest else {
        return String::new();
    };
    let Ok(content) = std::fs::read_to_string(latest.path()) else {
        return String::new();
    };
    content
        .lines()
        .filter(|line| line.contains("ERROR"))
        .rev()
        .take(max_lines)
        .map(redact)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Panic reports written by `crash_reporter::init` — already narrow
/// (timestamp, panic message, file/line), redacted defensively before
/// inclusion since a panic payload can in principle `Display` arbitrary data.
fn collect_crash_reports(crash_dir: &Path) -> String {
    let Ok(entries) = std::fs::read_dir(crash_dir) else {
        return String::new();
    };
    let mut reports: Vec<(std::time::SystemTime, String)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str());
        // J5 fix: crash reports are now written encrypted (`.enc`) — `.log`
        // is still read for any file left over from before that fix.
        let content = match ext {
            Some("enc") => std::fs::read(&path)
                .ok()
                .and_then(|bytes| crate::crash_reporter::decrypt_report(&bytes)),
            Some("log") => std::fs::read_to_string(&path).ok(),
            _ => None,
        };
        if let (Ok(meta), Some(content)) = (entry.metadata(), content) {
            let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            reports.push((modified, redact(&content)));
        }
    }
    reports.sort_by(|a, b| b.0.cmp(&a.0));
    reports
        .into_iter()
        .take(20)
        .map(|(_, c)| c)
        .collect::<Vec<_>>()
        .join("\n---\n")
}

/// Doc 19 §21.1 "Bundle includes: ... row counts per table (no raw financial
/// payloads)" — table names are read from `sqlite_master` (never hardcoded),
/// so this stays correct as the schema evolves, and only `COUNT(*)` is ever
/// executed, never a `SELECT *`.
fn collect_table_row_counts(conn: &Connection) -> Result<String> {
    let mut stmt = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
    )?;
    let table_names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();

    let mut out = String::new();
    for table in table_names {
        let count: i64 = conn
            .query_row(&format!("SELECT count(*) FROM \"{}\"", table), [], |row| {
                row.get(0)
            })
            .unwrap_or(0);
        out.push_str(&format!("{}: {}\n", table, count));
    }
    Ok(out)
}

/// Doc 19 §21.1, Doc 36 §4, Doc 41 §5: generates a single privacy-safe,
/// redacted diagnostic bundle (`.zip`) — the one mechanism for crash reports,
/// error logs, and schema/environment metadata, replacing the two previously
/// overlapping raw-text writers (`crash_reporter`, `feedback`'s unused
/// `include_logs` flag). Excludes, by construction: raw transaction/statement
/// data (never queried — only `COUNT(*)`), Gmail content, OAuth tokens,
/// PDFs, and full application logs (only redacted ERROR-level lines).
pub fn generate_diagnostic_bundle(
    app_data_dir: &Path,
    conn: &Connection,
    feedback_text: Option<&str>,
) -> Result<PathBuf> {
    let exports_dir = app_data_dir.join("exports");
    std::fs::create_dir_all(&exports_dir).context("Failed to create exports directory")?;

    let schema_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap_or(0);

    let mut sys = sysinfo::System::new_all();
    sys.refresh_all();
    let ram_gb = sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);

    let manifest = format!(
        "Dinero Diagnostic Bundle\n\
         Generated: {}\n\
         App Version: {}\n\
         OS: {} {}\n\
         RAM: {:.1} GB\n\
         Schema Version: {}\n",
        Local::now().to_rfc3339(),
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        ram_gb,
        schema_version,
    );

    let table_counts = collect_table_row_counts(conn).unwrap_or_default();

    // Matches lib.rs's exact log-directory resolution — the appender is
    // rooted at the current working directory (or its parent, if launched
    // from `src-tauri/`), not `app_data_dir`; this is a pre-existing, unrelated
    // quirk (see M48) reproduced here only so the bundle finds the real file.
    let mut log_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if log_dir.ends_with("src-tauri") {
        log_dir = log_dir.parent().unwrap_or(Path::new(".")).to_path_buf();
    }
    let error_lines = collect_error_lines(&log_dir, 500);

    let crash_dir = app_data_dir.join("audit_log").join("crash_reports");
    let crash_reports = collect_crash_reports(&crash_dir);

    let timestamp = Local::now().format("%Y-%m-%d").to_string();
    let zip_path = exports_dir.join(format!("dinero-diagnostic-logs-{}.zip", timestamp));
    let zip_file = std::fs::File::create(&zip_path).context("Failed to create bundle file")?;
    let mut zip = zip::ZipWriter::new(zip_file);
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("manifest.txt", options)?;
    zip.write_all(manifest.as_bytes())?;

    // Doc 41 §5: "Submit Feedback" attaches the user's own note to the same
    // bundle rather than a separate raw-text file — one mechanism, one
    // export flow. User-authored feedback text is not redacted (it isn't
    // extracted financial data, and redacting a user's own words they chose
    // to type would be actively unhelpful to support).
    if let Some(text) = feedback_text {
        zip.start_file("feedback.txt", options)?;
        zip.write_all(text.as_bytes())?;
    }

    zip.start_file("table_row_counts.txt", options)?;
    zip.write_all(table_counts.as_bytes())?;

    zip.start_file("error_log.txt", options)?;
    zip.write_all(error_lines.as_bytes())?;

    zip.start_file("crash_reports.txt", options)?;
    zip.write_all(crash_reports.as_bytes())?;

    zip.finish().context("Failed to finalize bundle zip")?;

    // Doc 25 §4.2/§4.4, Doc 36 §4 (C20 fix): bundle export is the one path by
    // which any operational data leaves the device — record it as a consent
    // event, same as Gmail authorization. Both callers (export_logs,
    // submit_user_feedback with include_logs) route through this single
    // function, so this is the one place that needs it.
    if let Err(e) = crate::auth::consent::insert_consent_event(
        conn,
        "bundle_export",
        &format!("User exported diagnostic bundle: {}", zip_path.display()),
    ) {
        tracing::warn!("Failed to record bundle_export consent event: {}", e);
    }

    Ok(zip_path)
}
