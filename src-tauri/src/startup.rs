//! TASK-SETUP-006: macOS RAM check on app startup.
//!
//! Queries total system RAM once at launch, sets the local-LLM eligibility
//! flag app-wide state consumes (TASK-TXN-006's extraction Layer 5 gate,
//! Document 16 §12.3's five-tier hardware matrix), and emits a
//! `system_warning` event (Document 19 §15.1) if RAM is below 8 GB. Must
//! never block or fail startup — the local LLM fallback is strictly
//! optional (Document 15 §9).

use tauri::{AppHandle, Emitter, Manager};

/// RAM below this threshold triggers a `system_warning` — Layer 5 extraction
/// is not offered at all below this line (Document 16 §12.3 tier 1 floor).
const LOW_RAM_WARNING_THRESHOLD_GB: f64 = 8.0;

/// RAM at or above this threshold auto-enables the local LLM fallback.
/// Below it (but at/above the warning floor), the 8-16GB tier's smaller
/// models remain available only via an explicit manual settings override
/// (TASK-TXN-006 wires the override read; this flag is the RAM-only default).
const LLM_AUTO_ELIGIBLE_THRESHOLD_GB: f64 = 16.0;

/// App-managed state read by the extraction pipeline (Layer 5) to decide
/// whether the local Candle `.gguf` fallback may run at all.
#[derive(Debug, Clone, Copy)]
pub struct LlmEligibility {
    pub eligible: bool,
    pub total_ram_gb: f64,
}

/// Pure function over a RAM figure — kept separate from the `sysinfo` call
/// so the threshold logic is unit-testable without depending on the host
/// machine's actual RAM.
pub fn compute_llm_eligibility(total_ram_gb: f64) -> LlmEligibility {
    LlmEligibility {
        eligible: total_ram_gb >= LLM_AUTO_ELIGIBLE_THRESHOLD_GB,
        total_ram_gb,
    }
}

/// Reads total system RAM via `sysinfo`, manages `LlmEligibility` as Tauri
/// app state, and emits `system_warning` if RAM is below the warning floor.
/// Infallible by design — a RAM-check failure must never block startup.
pub fn check_ram_and_set_llm_eligibility(app: &AppHandle) {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_memory();
    let total_ram_gb = sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);

    let eligibility = compute_llm_eligibility(total_ram_gb);
    app.manage(eligibility);

    if total_ram_gb < LOW_RAM_WARNING_THRESHOLD_GB {
        tracing::warn!(
            "Low system RAM detected: {:.1} GB (< {} GB floor)",
            total_ram_gb,
            LOW_RAM_WARNING_THRESHOLD_GB
        );
        let _ = app.emit(
            crate::ipc::events::AppEvent::SystemWarning.as_str(),
            serde_json::json!({
                "warning_type": "low_ram",
                "message": format!(
                    "Your Mac has {:.1} GB of RAM, below the {} GB recommended minimum. \
                    Some features may run more slowly.",
                    total_ram_gb, LOW_RAM_WARNING_THRESHOLD_GB
                ),
                "available_gb": total_ram_gb,
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_ram_is_not_llm_eligible() {
        let e = compute_llm_eligibility(4.0);
        assert!(!e.eligible);
    }

    #[test]
    fn mid_tier_8_to_16gb_is_not_auto_eligible() {
        // 8-16GB tier requires a manual override (TASK-TXN-006), not auto-enabled.
        let e = compute_llm_eligibility(12.0);
        assert!(!e.eligible);
    }

    #[test]
    fn sixteen_gb_is_eligible() {
        let e = compute_llm_eligibility(16.0);
        assert!(e.eligible);
    }

    #[test]
    fn high_ram_is_eligible() {
        let e = compute_llm_eligibility(64.0);
        assert!(e.eligible);
    }

    #[test]
    fn boundary_just_below_sixteen_is_not_eligible() {
        let e = compute_llm_eligibility(15.99);
        assert!(!e.eligible);
    }
}
