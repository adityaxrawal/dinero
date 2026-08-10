//! Builds the diagnostic bundle a user can attach to a support request.
//!
//! The defining constraint is that this data leaves the machine, so the bundle
//! is assembled from redacted sources and then re-checked before it is written.
//! `scan_for_pii` is that second gate: it greps the assembled text for
//! email-shaped, card-shaped and currency-shaped matches and aborts the export
//! outright if any are found.
//!
//! Failing the export is deliberate. A bundle that cannot be produced is a
//! nuisance; one that quietly carries a card number is a breach, so the check
//! errors rather than stripping and continuing.

use anyhow::{Context, Result};
use chrono::Local;
use regex::Regex;
use rusqlite::Connection;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::logging::redact;

/// Final safety net before a bundle is written to disk.
///
/// Deliberately pattern-based and conservative: it looks for the *shape* of
/// sensitive data rather than known values, so it catches leaks from sources
/// this module does not know about. A match aborts the export instead of
/// redacting, because a silent redaction would hide the fact that some upstream
/// redaction failed.
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

/// Collects recent error lines from the logs, bounded in length.
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

/// Collects crash reports, decrypting them for inclusion.
fn collect_crash_reports(crash_dir: &Path) -> String {
    let Ok(entries) = std::fs::read_dir(crash_dir) else {
        return String::new();
    };
    let mut reports: Vec<(std::time::SystemTime, String)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str());
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
    reports.sort_by_key(|r| std::cmp::Reverse(r.0));
    reports
        .into_iter()
        .take(20)
        .map(|(_, c)| c)
        .collect::<Vec<_>>()
        .join("\n---\n")
}

/// Collects per-table row counts.
///
/// Counts only -- never contents -- so the bundle describes the database's shape
/// without carrying any of the user's financial data.
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

/// Summarises pipeline health for the bundle.
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

/// Collects audit entries with sensitive fields redacted.
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

/// Assembles the diagnostic bundle, refusing to write it if PII is detected.
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

    let mut log_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if log_dir.ends_with("src-tauri") {
        log_dir = log_dir.parent().unwrap_or(Path::new(".")).to_path_buf();
    }
    let error_lines = collect_error_lines(&log_dir, 500);

    let crash_dir = app_data_dir.join("audit_log").join("crash_reports");
    let crash_reports = collect_crash_reports(&crash_dir);
    let pipeline_health = collect_pipeline_health(conn);
    let audit_log_redacted = collect_redacted_audit_log(conn);

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

    #[test]
    fn scan_for_pii_rejects_email_card_and_amount() {
        assert!(scan_for_pii(&[("x", "reach me at test@example.com")]).is_err());
        assert!(scan_for_pii(&[("x", "card 4111-1111-1111-1111")]).is_err());
        assert!(scan_for_pii(&[("x", "total ₹1,234.56")]).is_err());
        assert!(scan_for_pii(&[("x", "no PII in here, just a count: 12")]).is_ok());
    }

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

        assert!(
            zip_path.exists(),
            "the bundle must be written to a real local file"
        );
        assert!(
            zip_path.starts_with(&temp_dir),
            "the bundle must be written under the app data dir, never a network location"
        );

        let file = std::fs::File::open(&zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut feedback = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("feedback.txt").unwrap(), &mut feedback)
            .unwrap();
        assert_eq!(feedback, "a note from the user");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_crash_bundle_excludes_sensitive_fields() {
        let conn = crate::db::test_helpers::setup_test_db();
        let temp_dir =
            std::env::temp_dir().join(format!("dinero_crash_bundle_test_{}", uuid::Uuid::new_v4()));
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
