//! Hardware inspection and the local-LLM sizing decisions derived from it.
//!
//! Runs during startup to answer three questions from the machine's RAM and core
//! count: whether local inference is viable at all, which model to recommend,
//! and how many requests may run in parallel.
//!
//! The concurrency maths is deliberately conservative. Each parallel slot holds
//! its own copy of the model's working set, so slots are bounded by available
//! memory before CPU -- overcommitting there does not merely run slowly, it
//! pushes the machine into swap and can take the whole app down with it.

use tauri::{AppHandle, Manager};

const LOW_RAM_WARNING_THRESHOLD_GB: f64 = 8.0;

const LLM_AUTO_ELIGIBLE_THRESHOLD_GB: f64 = LOW_RAM_WARNING_THRESHOLD_GB;

#[derive(Debug, Clone, Copy)]
pub struct LlmEligibility {
    pub eligible: bool,
    pub total_ram_gb: f64,
}

/// Whether this machine can run local inference at all.
pub fn compute_llm_eligibility(total_ram_gb: f64) -> LlmEligibility {
    LlmEligibility {
        eligible: total_ram_gb >= LLM_AUTO_ELIGIBLE_THRESHOLD_GB,
        total_ram_gb,
    }
}

/// Checks RAM at launch and records LLM eligibility.
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
        crate::ipc::system_warnings::emit_system_warning(
            app,
            crate::ipc::system_warnings::SystemWarningPayload {
                warning_type: "low_ram".to_string(),
                message: format!(
                    "Your Mac has {:.1} GB of RAM, below the {} GB recommended minimum. \
                    Some features may run more slowly.",
                    total_ram_gb, LOW_RAM_WARNING_THRESHOLD_GB
                ),
                severity: crate::ipc::system_warnings::WarningSeverity::Info,
                action_hint: None,
            },
        );
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct HardwareInfo {
    pub total_ram_gb: f64,
    pub cpu_cores: usize,
}

/// Reads RAM and core count for sizing decisions.
pub fn read_hardware_info() -> HardwareInfo {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_memory();
    sys.refresh_cpu();
    HardwareInfo {
        total_ram_gb: sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0),
        cpu_cores: sys.physical_core_count().unwrap_or(4),
    }
}

// ponytail: unmeasured heuristic constants, not benchmarked KV-cache costs.
const OS_RESERVE_GB: f64 = 4.0;
const PER_SLOT_KV_GB: f64 = 1.0;
const CORES_PER_SLOT: usize = 2;
const MAX_RECOMMENDED_SLOTS: usize = 10;

/// Hard ceiling on parallel slots for a given model and machine.
///
/// Memory-bound rather than CPU-bound: exceeding it means swapping, which is far
/// worse than simply queuing requests.
pub fn max_safe_slots(ram_gb: f64, model_size_gb: f64) -> usize {
    let ram_budget = ((ram_gb - model_size_gb - OS_RESERVE_GB) / PER_SLOT_KV_GB).floor();
    (ram_budget.max(1.0) as usize).min(MAX_RECOMMENDED_SLOTS)
}

/// Suggested slot count, respecting both the memory ceiling and the core count.
///
/// The recommendation is the smaller of the two, so neither resource is
/// oversubscribed.
pub fn compute_recommended_slots(ram_gb: f64, cpu_cores: usize, model_size_gb: f64) -> usize {
    let cpu_budget = (cpu_cores / CORES_PER_SLOT).max(1);
    max_safe_slots(ram_gb, model_size_gb).min(cpu_budget)
}

/// Largest model this machine can comfortably run, or None if none fit.
pub fn recommend_model_id(ram_gb: f64) -> Option<String> {
    let budget = ram_gb - OS_RESERVE_GB;
    crate::llm_manager::get_available_models()
        .into_iter()
        .filter(|m| m.min_ram_gb <= budget)
        .max_by_key(|m| m.tier)
        .map(|m| m.id)
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
    fn mid_tier_8gb_and_up_is_now_eligible() {
        let e = compute_llm_eligibility(12.0);
        assert!(e.eligible);
    }

    #[test]
    fn eight_gb_is_eligible() {
        let e = compute_llm_eligibility(8.0);
        assert!(e.eligible);
    }

    #[test]
    fn below_eight_gb_is_not_eligible() {
        let e = compute_llm_eligibility(7.99);
        assert!(!e.eligible);
    }

    #[test]
    fn high_ram_is_eligible() {
        let e = compute_llm_eligibility(64.0);
        assert!(e.eligible);
    }

    #[test]
    fn max_safe_slots_bounds_the_override_by_ram_not_by_cores() {
        assert_eq!(max_safe_slots(16.0, 8.0), 4);

        assert_eq!(compute_recommended_slots(16.0, 2, 8.0), 1);
        assert!(max_safe_slots(16.0, 8.0) >= compute_recommended_slots(16.0, 2, 8.0));

        assert_eq!(max_safe_slots(9.5, 5.0), 1);
        assert_eq!(max_safe_slots(8.0, 5.0), 1);

        assert_eq!(max_safe_slots(256.0, 5.0), MAX_RECOMMENDED_SLOTS);
    }

    #[test]
    fn recommended_slots_clamps_to_1_when_ram_budget_negative() {
        assert_eq!(compute_recommended_slots(8.0, 8, 5.0), 1);
    }

    #[test]
    fn recommended_slots_clamps_on_cpu_budget_when_cores_low() {
        assert_eq!(compute_recommended_slots(64.0, 2, 5.0), 1);
    }

    #[test]
    fn recommended_slots_clamps_at_max_10_when_both_budgets_are_high() {
        assert_eq!(compute_recommended_slots(256.0, 40, 5.0), 10);
    }

    #[test]
    fn recommended_slots_never_zero_at_the_ram_floor() {
        assert_eq!(compute_recommended_slots(9.5, 8, 5.0), 1);
    }

    #[test]
    fn recommended_slots_realistic_mid_tier_machine() {
        assert_eq!(compute_recommended_slots(24.0, 10, 5.0), 5);
    }

    #[test]
    fn recommend_model_id_picks_highest_fitting_tier() {
        assert_eq!(recommend_model_id(40.0), Some("gemma4_31b".to_string()));
    }

    #[test]
    fn recommend_model_id_picks_mid_tier_when_top_tiers_dont_fit() {
        assert_eq!(recommend_model_id(20.0), Some("qwen3_6_27b".to_string()));
    }

    #[test]
    fn recommend_model_id_falls_back_to_smallest_tier() {
        assert_eq!(recommend_model_id(12.0), Some("gemma4_e4b".to_string()));
    }

    #[test]
    fn recommend_model_id_returns_none_below_every_tier_floor() {
        assert_eq!(recommend_model_id(5.0), None);
    }
}
