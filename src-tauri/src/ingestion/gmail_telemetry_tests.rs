use crate::ingestion::gmail_telemetry::GmailTelemetry;
use std::time::Duration;

#[test]
fn test_quota_exhausted_count_increments() {
    let t = GmailTelemetry::default();
    t.record_quota_exhausted();
    t.record_quota_exhausted();
    assert_eq!(t.snapshot().quota_exhausted_count, 2);
}

#[test]
fn test_5xx_counted_per_status_code() {
    let t = GmailTelemetry::default();
    t.record_5xx(500);
    t.record_5xx(500);
    t.record_5xx(503);
    let snap = t.snapshot();
    assert_eq!(snap.error_5xx_by_status.get(&500), Some(&2));
    assert_eq!(snap.error_5xx_by_status.get(&503), Some(&1));
}

#[test]
fn test_poll_cycle_duration_averages() {
    let t = GmailTelemetry::default();
    t.record_poll_cycle_duration(Duration::from_millis(100));
    t.record_poll_cycle_duration(Duration::from_millis(300));
    assert_eq!(t.snapshot().avg_poll_cycle_duration_ms, 200.0);
}

/// Doc 30 TASK-GMAIL-010: "per-gate counts" — proves each gate's rejections
/// are tracked independently rather than merged into one bucket, which is
/// what makes "a bank changed its template and Gate 1 rejections spiked"
/// distinguishable from a Gate 2/3 problem.
#[test]
fn test_gate_rejection_rate_tracked_per_gate() {
    let t = GmailTelemetry::default();
    t.record_gate_rejection("gate1");
    t.record_gate_rejection("gate1");
    t.record_gate_rejection("gate2");
    let snap = t.snapshot();
    assert_eq!(snap.gate_rejections.get("gate1"), Some(&2));
    assert_eq!(snap.gate_rejections.get("gate2"), Some(&1));
    assert_eq!(snap.gate_rejections.get("gate3"), None);
}

/// Doc 30 TASK-GMAIL-010: "all telemetry payloads exclude email content,
/// sender addresses, subjects, and transaction data." Enforced at the type
/// level — `GmailTelemetrySnapshot`'s fields are only counts, status codes,
/// gate-name labels, and a duration average; there is no `String` field
/// capable of carrying free-form content for a caller to accidentally fill in.
#[test]
fn test_snapshot_shape_carries_no_content_fields() {
    let t = GmailTelemetry::default();
    t.record_quota_exhausted();
    t.record_5xx(502);
    t.record_poll_cycle_duration(Duration::from_millis(50));
    t.record_gate_rejection("gate2");

    let snap = t.snapshot();
    let json = serde_json::to_value(&snap).unwrap();
    let obj = json.as_object().unwrap();

    // Every value in the serialized snapshot must be a number, or a map/object
    // whose own keys/values are themselves numbers or short gate-name labels
    // — never a string long enough to be an email subject, address, or body.
    fn assert_no_long_strings(value: &serde_json::Value) {
        match value {
            serde_json::Value::String(s) => {
                assert!(
                    s.len() <= "quota_exhausted".len(),
                    "unexpectedly long string in telemetry snapshot: {:?}",
                    s
                );
            }
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    assert!(k.len() <= "avg_poll_cycle_duration_ms".len());
                    assert_no_long_strings(v);
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    assert_no_long_strings(v);
                }
            }
            _ => {}
        }
    }

    for (_, v) in obj {
        assert_no_long_strings(v);
    }
}
