use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// Doc 30 TASK-GMAIL-010: privacy-safe structured telemetry for the Gmail
/// ingestion pipeline (poll worker, message fetch, historical scan). Every
/// field is an aggregate count, a status code, a gate-name label, or a
/// duration — never email content, sender addresses, subjects, or
/// transaction data, consistent with the strict on-device telemetry model
/// (Document 06 §5). `GmailTelemetrySnapshot` (the only thing ever read back
/// out) enforces this shape at the type level: it has nowhere to put
/// free-form content even if a future caller tried.
#[derive(Default)]
pub struct GmailTelemetry {
    /// gmail_api_quota_exhausted: 429 responses.
    quota_exhausted_count: AtomicU64,
    /// gmail_api_error_5xx: count per HTTP status code, never a response body.
    error_5xx_by_status: Mutex<HashMap<u16, u64>>,
    /// gmail_poll_cycle_duration_ms: running count/sum, exposed as an average.
    poll_cycle_count: AtomicU64,
    poll_cycle_total_ms: AtomicU64,
    /// gmail_gate_rejection_rate: per-gate rejection counts ("gate1"/"gate2"/"gate3").
    gate_rejections: Mutex<HashMap<String, u64>>,
}

impl GmailTelemetry {
    pub fn record_quota_exhausted(&self) {
        self.quota_exhausted_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_5xx(&self, status: u16) {
        let mut map = self.error_5xx_by_status.lock().unwrap();
        *map.entry(status).or_insert(0) += 1;
    }

    pub fn record_poll_cycle_duration(&self, duration: Duration) {
        self.poll_cycle_count.fetch_add(1, Ordering::Relaxed);
        self.poll_cycle_total_ms
            .fetch_add(duration.as_millis() as u64, Ordering::Relaxed);
    }

    pub fn record_gate_rejection(&self, gate: &str) {
        let mut map = self.gate_rejections.lock().unwrap();
        *map.entry(gate.to_string()).or_insert(0) += 1;
    }

    pub fn snapshot(&self) -> GmailTelemetrySnapshot {
        let poll_cycle_count = self.poll_cycle_count.load(Ordering::Relaxed);
        let avg_poll_cycle_duration_ms = if poll_cycle_count == 0 {
            0.0
        } else {
            self.poll_cycle_total_ms.load(Ordering::Relaxed) as f64 / poll_cycle_count as f64
        };

        GmailTelemetrySnapshot {
            quota_exhausted_count: self.quota_exhausted_count.load(Ordering::Relaxed),
            error_5xx_by_status: self.error_5xx_by_status.lock().unwrap().clone(),
            avg_poll_cycle_duration_ms,
            gate_rejections: self.gate_rejections.lock().unwrap().clone(),
        }
    }
}

/// Aggregate-only snapshot — safe to hand to the debug dashboard (TASK-QA-007's
/// privacy regression suite is the authoritative check that nothing content-shaped
/// ever gets added here) or a future support-bundle export.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct GmailTelemetrySnapshot {
    pub quota_exhausted_count: u64,
    pub error_5xx_by_status: HashMap<u16, u64>,
    pub avg_poll_cycle_duration_ms: f64,
    pub gate_rejections: HashMap<String, u64>,
}

/// Process-wide singleton, same pattern as `gmail_client::full_fetch_semaphore` —
/// avoids threading a Tauri-managed state handle through `MessageProcessor::process_message`
/// (called identically from both the real-time poll path and the historical
/// scan path, neither of which currently carries an `AppHandle` down this far).
pub fn gmail_telemetry() -> &'static GmailTelemetry {
    static TELEMETRY: OnceLock<GmailTelemetry> = OnceLock::new();
    TELEMETRY.get_or_init(GmailTelemetry::default)
}
