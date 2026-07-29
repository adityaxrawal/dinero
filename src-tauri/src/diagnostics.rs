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
///
/// TASK-OPS-007: moved to `crate::logging` so the same regexes back both
/// this lazy bundle-export pass and `RedactingWriter`'s write-time pass over
/// `app-logs.log` itself, rather than drifting into two copies.
use crate::logging::redact;

/// TASK-AUTH-015's acceptance criteria: "An automated test scans the
/// generated bundle for zero PII matches (email/16-digit-card/₹-amount
/// regex) before allowing export to proceed." Implemented as a real runtime
/// gate here, not only a test — even if a future change introduces a bug in
/// `redact()`, this final verification pass over the assembled bundle
/// content refuses to write the ZIP rather than silently shipping PII.
/// Returns `Err` naming the first pattern that still matches.
fn scan_for_pii(sections: &[(&str, &str)]) -> Result<()> {
    let email_re = Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}").unwrap();
    let card_re = Regex::new(r"\b\d{4}[\s-]?\d{4}[\s-]?\d{4}[\s-]?\d{4}\b").unwrap();
    let rupee_amount_re = Regex::new(r"₹\s?[\d,]+(\.\d{1,2})?").unwrap();

    for (name, content) in sections {
        if email_re.is_match(content) {
            anyhow::bail!(
                "PII scan blocked export: email-shaped match found in '{}'",
                name
            );
        }
        if card_re.is_match(content) {
            anyhow::bail!(
                "PII scan blocked export: card-number-shaped match found in '{}'",
                name
            );
        }
        if rupee_amount_re.is_match(content) {
            anyhow::bail!(
                "PII scan blocked export: ₹-amount-shaped match found in '{}'",
                name
            );
        }
    }
    Ok(())
}

/// Doc 19 §21.1's "Bundle includes: ... error traces and panic logs
/// (redacted of PII and financial data)" — only ERROR-level lines from the
/// rolling app log are considered (an aggressive whitelist: most financial
/// data flows through info/debug-level pipeline logs, not error logs), each
/// additionally passed through `redact`.
fn collect_error_lines(log_dir: &Path, max_lines: usize) -> String {
    let target_dir = if log_dir.join("logs").is_dir() {
        log_dir.join("logs")
    } else {
        log_dir.to_path_buf()
    };
    let Ok(entries) = std::fs::read_dir(&target_dir) else {
        return String::new();
    };
    let latest = entries
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| {
                    n.starts_with("combined.log")
                        || n.starts_with("backend.log")
                        || n.starts_with("app-logs.log")
                })
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

/// TASK-AUTH-015: "pipeline health metrics" in the bundle — reuses the same
/// aggregate-only metrics the Debug page already computes (`do_get_debug_metrics`:
/// extraction-layer distribution, reconciliation-decision distribution, LLM
/// fallback rate, queue depth). No raw transaction/merchant content, only
/// counts and rates.
fn collect_pipeline_health(conn: &Connection) -> String {
    match crate::commands::data::do_get_debug_metrics(conn) {
        Ok(metrics) => format!(
            "total_transactions: {}\ntotal_statements: {}\nunresolved_clusters: {}\nllm_fallback_rate: {:.3}\nqueue_depth: {}\nextraction_layer_distribution: {:?}\nreconciliation_decision_distribution: {:?}\n",
            metrics.total_transactions,
            metrics.total_statements,
            metrics.unresolved_clusters,
            metrics.llm_fallback_rate,
            metrics.queue_depth,
            metrics.extraction_layer_distribution,
            metrics.reconciliation_decision_distribution,
        ),
        Err(e) => format!("Failed to collect pipeline health metrics: {}\n", e),
    }
}

/// TASK-AUTH-015: "redacted audit-log entries" — only `actor_type`,
/// `action`, `resource_type`, and `created_at` are included verbatim (all
/// non-sensitive category/timestamp fields); `before_json`/`after_json` (the
/// fields most likely to carry real values — email addresses, file paths,
/// consent text) are passed through `redact` rather than included raw or
/// omitted outright, since the redacted shape is still useful for support to
/// see *what kind* of change occurred.
fn collect_redacted_audit_log(conn: &Connection) -> String {
    let Ok(entries) = crate::db::audit_log::fetch_all(conn, None, 200, 0) else {
        return String::new();
    };
    entries
        .into_iter()
        .map(|e| {
            format!(
                "{} | actor={} | action={} | resource={} | after={}",
                e.created_at.to_rfc3339(),
                e.actor_type.unwrap_or_default(),
                e.action.unwrap_or_default(),
                e.resource_type.unwrap_or_default(),
                redact(&e.after_json.map(|v| v.to_string()).unwrap_or_default()),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
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
    let pipeline_health = collect_pipeline_health(conn);
    let audit_log_redacted = collect_redacted_audit_log(conn);

    // TASK-AUTH-015 acceptance criterion: block export on any PII match, as
    // a real runtime gate over the assembled content — not only a test.
    // `feedback_text` is deliberately excluded (see the comment below: it's
    // the user's own words, not extracted financial data, and isn't subject
    // to this scan).
    scan_for_pii(&[
        ("manifest", &manifest),
        ("table_row_counts", &table_counts),
        ("error_log", &error_lines),
        ("crash_reports", &crash_reports),
        ("pipeline_health", &pipeline_health),
        ("audit_log_redacted", &audit_log_redacted),
    ])?;

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

    zip.start_file("pipeline_health.txt", options)?;
    zip.write_all(pipeline_health.as_bytes())?;

    zip.start_file("audit_log_redacted.txt", options)?;
    zip.write_all(audit_log_redacted.as_bytes())?;

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

#[cfg(test)]
mod tests {
    use super::*;

    // `redact()`'s own regex-behavior tests now live in `crate::logging`
    // (moved there under TASK-OPS-007, since that's where the function
    // itself now lives) -- this module's tests below cover what's specific
    // to *this* module: the PII-scan gate and the full bundle-generation flow.

    #[test]
    fn scan_for_pii_rejects_email_card_and_amount() {
        assert!(scan_for_pii(&[("x", "reach me at test@example.com")]).is_err());
        assert!(scan_for_pii(&[("x", "card 4111-1111-1111-1111")]).is_err());
        assert!(scan_for_pii(&[("x", "total ₹1,234.56")]).is_err());
        assert!(scan_for_pii(&[("x", "no PII in here, just a count: 12")]).is_ok());
    }

    /// TASK-AUTH-015's exact acceptance criterion: "An automated test scans
    /// the generated bundle for zero PII matches ... before allowing export
    /// to proceed." This generates a real bundle against a real (seeded,
    /// PII-bearing) database, then unzips every file it produced and scans
    /// the actual bytes on disk — not just the in-memory strings the
    /// function assembled — for the same three patterns.
    #[test]
    fn generated_bundle_contains_zero_pii_matches() {
        let conn = crate::db::test_helpers::setup_test_db();
        conn.execute(
            "INSERT INTO instruments (id, type, issuer_name, masked_identifier, status) \
             VALUES ('inst_1', 'credit_card', 'HDFC', '1234', 'active')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO local_profile (id, primary_email) VALUES (1, 'realuser@example.com')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions (id, instrument_id, amount_minor, currency, direction, merchant_display_name, is_deleted) \
             VALUES ('tx_1', 'inst_1', 49900, 'INR', 'debit', 'Some Merchant ₹499.00', 0)",
            [],
        )
        .unwrap();
        crate::db::audit_log::insert(
            &conn,
            &crate::db::audit_log::AuditLogRow {
                id: "audit_1".to_string(),
                actor_type: Some("user".to_string()),
                actor_id: Some("local".to_string()),
                action: Some("consent_gmail_oauth_consent".to_string()),
                resource_type: Some("consent".to_string()),
                resource_id: None,
                before_json: None,
                after_json: Some(serde_json::json!({
                    "email": "realuser@example.com",
                    "card_seen": "4111111111111111",
                    "amount": "₹499.00",
                })),
                created_at: chrono::Utc::now(),
            },
        )
        .unwrap();

        let temp_dir =
            std::env::temp_dir().join(format!("dinero_bundle_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let zip_path = generate_diagnostic_bundle(&temp_dir, &conn, None).unwrap();

        let file = std::fs::File::open(&zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).unwrap();
            let name = entry.name().to_string();
            let mut contents = String::new();
            std::io::Read::read_to_string(&mut entry, &mut contents).unwrap();
            scan_for_pii(&[(&name, &contents)])
                .unwrap_or_else(|e| panic!("PII leaked into bundle file '{}': {}", name, e));
        }

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    /// Doc 30 TASK-OPS-004 acceptance: `test_diagnostic_bundle_contains_version_metadata`.
    #[test]
    fn test_diagnostic_bundle_contains_version_metadata() {
        let conn = crate::db::test_helpers::setup_test_db();
        let temp_dir = std::env::temp_dir().join(format!(
            "dinero_bundle_version_test_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let zip_path = generate_diagnostic_bundle(&temp_dir, &conn, None).unwrap();
        let file = std::fs::File::open(&zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut manifest = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("manifest.txt").unwrap(), &mut manifest)
            .unwrap();

        assert!(manifest.contains(&format!("App Version: {}", env!("CARGO_PKG_VERSION"))));
        assert!(manifest.contains(&format!("OS: {}", std::env::consts::OS)));
        assert!(manifest.contains("Schema Version:"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    /// Doc 30 TASK-OPS-004 acceptance: `test_support_export_writes_local_archive`.
    /// The one export path (`export_logs` IPC command / "Submit Feedback"
    /// with `include_logs`) writes a real local `.zip` -- entirely on-device,
    /// never uploaded anywhere by this function itself.
    #[test]
    fn test_support_export_writes_local_archive() {
        let conn = crate::db::test_helpers::setup_test_db();
        let temp_dir = std::env::temp_dir().join(format!(
            "dinero_bundle_export_test_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let zip_path =
            generate_diagnostic_bundle(&temp_dir, &conn, Some("a note from the user")).unwrap();

        assert!(zip_path.exists(), "the bundle must be written to a real local file");
        assert!(zip_path.starts_with(&temp_dir), "the bundle must be written under the app data dir, never a network location");

        let file = std::fs::File::open(&zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut feedback = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("feedback.txt").unwrap(), &mut feedback)
            .unwrap();
        assert_eq!(feedback, "a note from the user");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    /// Doc 30 TASK-OPS-004 acceptance: `test_crash_bundle_excludes_sensitive_fields`.
    /// A crash report containing a raw email/card/amount (the kind of thing a
    /// panic's `Display` output could in principle carry) must never reach
    /// the bundle un-redacted.
    #[test]
    fn test_crash_bundle_excludes_sensitive_fields() {
        let conn = crate::db::test_helpers::setup_test_db();
        let temp_dir = std::env::temp_dir().join(format!(
            "dinero_crash_bundle_test_{}",
            uuid::Uuid::new_v4()
        ));
        let crash_dir = temp_dir.join("audit_log").join("crash_reports");
        std::fs::create_dir_all(&crash_dir).unwrap();

        let sensitive_report = "Dinero App Crash Report\nPanic: reached user@example.com with card 4111 1111 1111 1111 for ₹4,999.00\n";
        std::fs::write(crash_dir.join("crash_test.log"), sensitive_report).unwrap();

        let zip_path = generate_diagnostic_bundle(&temp_dir, &conn, None).unwrap();
        let file = std::fs::File::open(&zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut crash_reports = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("crash_reports.txt").unwrap(),
            &mut crash_reports,
        )
        .unwrap();

        assert!(!crash_reports.contains("user@example.com"));
        assert!(!crash_reports.contains("4111 1111 1111 1111"));
        assert!(!crash_reports.contains("₹4,999.00"));
        assert!(crash_reports.contains("[REDACTED_EMAIL]"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
