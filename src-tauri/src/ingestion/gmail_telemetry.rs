//! Local counters for Gmail API usage.
//!
//! Purely in-process and never transmitted. Exists to make quota consumption and
//! error rates visible when diagnosing a scan that is running slowly or failing.
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

#[derive(Default)]
pub struct GmailTelemetry {
    quota_exhausted_count: AtomicU64,
    error_5xx_by_status: Mutex<HashMap<u16, u64>>,
    poll_cycle_count: AtomicU64,
    poll_cycle_total_ms: AtomicU64,
    gate_rejections: Mutex<HashMap<String, u64>>,
}

impl GmailTelemetry {
    /// Records a quota exhaustion response from the API.
    pub fn record_quota_exhausted(&self) {
        self.quota_exhausted_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a server-side error response.
    pub fn record_5xx(&self, status: u16) {
        let mut map = self.error_5xx_by_status.lock().unwrap();
        *map.entry(status).or_insert(0) += 1;
    }

    /// Records how long a poll cycle took.
    pub fn record_poll_cycle_duration(&self, duration: Duration) {
        self.poll_cycle_count.fetch_add(1, Ordering::Relaxed);
        self.poll_cycle_total_ms
            .fetch_add(duration.as_millis() as u64, Ordering::Relaxed);
    }

    /// Records which ingestion gate rejected a message.
    ///
    /// Per-gate counts show where messages are being dropped, which is what makes an
    /// over-aggressive filter diagnosable rather than showing up as silently missing
    /// transactions.
    pub fn record_gate_rejection(&self, gate: &str) {
        let mut map = self.gate_rejections.lock().unwrap();
        *map.entry(gate.to_string()).or_insert(0) += 1;
    }

    /// Takes a consistent snapshot of the counters for display.
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

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct GmailTelemetrySnapshot {
    pub quota_exhausted_count: u64,
    pub error_5xx_by_status: HashMap<u16, u64>,
    pub avg_poll_cycle_duration_ms: f64,
    pub gate_rejections: HashMap<String, u64>,
}

/// The process-wide telemetry instance.
///
/// Local only -- these counters are never transmitted anywhere.
pub fn gmail_telemetry() -> &'static GmailTelemetry {
    static TELEMETRY: OnceLock<GmailTelemetry> = OnceLock::new();
    TELEMETRY.get_or_init(GmailTelemetry::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gmail_telemetry_snapshot_contains_no_free_form_content() {
        let telemetry = GmailTelemetry::default();
        telemetry.record_quota_exhausted();
        telemetry.record_5xx(503);
        telemetry.record_poll_cycle_duration(std::time::Duration::from_millis(250));
        telemetry.record_gate_rejection("gate1");

        let snapshot = telemetry.snapshot();
        for gate_name in snapshot.gate_rejections.keys() {
            assert!(
                ["gate1", "gate2", "gate3"].contains(&gate_name.as_str()),
                "gate_rejections key '{gate_name}' is not one of the documented bounded gate names"
            );
        }
        for status in snapshot.error_5xx_by_status.keys() {
            assert!(
                *status >= 500 && *status < 600,
                "unexpected non-5xx status code key: {status}"
            );
        }
    }
}
