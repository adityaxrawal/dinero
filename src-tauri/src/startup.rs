//! TASK-SETUP-006: macOS RAM check on app startup.
//!
//! Queries total system RAM once at launch, sets the local-LLM eligibility
//! flag app-wide state consumes (TASK-TXN-006's extraction Layer 5 gate,
//! Document 16 §12.3's five-tier hardware matrix), and emits a
//! `system_warning` event (Document 19 §15.1) if RAM is below 8 GB. Must
//! never block or fail startup — the local LLM fallback is strictly
//! optional (Document 15 §9).

use tauri::{AppHandle, Manager};

/// RAM below this threshold triggers a `system_warning` — Layer 5 extraction
/// is not offered at all below this line (Document 16 §12.3 tier 1 floor).
const LOW_RAM_WARNING_THRESHOLD_GB: f64 = 8.0;

/// RAM below this threshold rules out even the catalog's smallest tier
/// (`gemma4_e4b`, `min_ram_gb: 8.0`) -- shares the same floor as the
/// low-RAM `system_warning` on purpose. Below it, Layer 6 categorically
/// cannot run regardless of which model the user picks; at or above it,
/// whether a model has actually been downloaded and selected is checked
/// where that information actually lives (`Layer6LlmLayer::run`), not
/// re-approximated here from RAM alone.
const LLM_AUTO_ELIGIBLE_THRESHOLD_GB: f64 = LOW_RAM_WARNING_THRESHOLD_GB;

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

/// App-wide hardware snapshot for the Settings "Parallel Processing" panel —
/// read once per Settings load via `sysinfo`, not cached, since RAM/core
/// count won't change mid-session on real hardware and re-reading is cheap.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct HardwareInfo {
    pub total_ram_gb: f64,
    pub cpu_cores: usize,
}

pub fn read_hardware_info() -> HardwareInfo {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_memory();
    sys.refresh_cpu();
    HardwareInfo {
        total_ram_gb: sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0),
        // `physical_core_count()` is an instance method requiring a refreshed
        // `System`, not the free/static function it looks like it should be —
        // defaults to 4 on the (essentially never-hit) platforms where sysinfo
        // can't determine it.
        cpu_cores: sys.physical_core_count().unwrap_or(4),
    }
}

// ponytail: unmeasured heuristic constants, not benchmarked KV-cache costs.
// Revisit if real-world OOMs or under-utilization show up in the field.
const OS_RESERVE_GB: f64 = 4.0;
const PER_SLOT_KV_GB: f64 = 1.0;
const CORES_PER_SLOT: usize = 2;
const MAX_RECOMMENDED_SLOTS: usize = 10;

/// The most slots this machine's RAM can hold for `model_size_gb`, ignoring
/// CPU entirely.
///
/// audit_06 #4: `llm_set_parallel_slots` clamped the user's override to a
/// fixed `1..=10` — the same 10 regardless of hardware — while
/// `llama_sidecar::context_size_for` multiplies the slot count into
/// `--context-size` (2048 per slot) and every slot also costs its own KV
/// cache. So a 16 GB machine running a 12B model, whose recommendation is 4,
/// could be set to 10 by hand and OOM `llama-server` at startup.
///
/// Split out from [`compute_recommended_slots`] rather than reusing it whole
/// because the two answer different questions: the CPU term is a throughput
/// *recommendation* (more slots than cores just thrashes), while the RAM term
/// is a hard *ceiling* (more slots than memory crashes). Clamping a manual
/// override to the CPU term would take away an override the user is entitled
/// to make; clamping it to the RAM term stops a crash.
pub fn max_safe_slots(ram_gb: f64, model_size_gb: f64) -> usize {
    let ram_budget = ((ram_gb - model_size_gb - OS_RESERVE_GB) / PER_SLOT_KV_GB).floor();
    (ram_budget.max(1.0) as usize).min(MAX_RECOMMENDED_SLOTS)
}

/// Recommended parallel `llama-server` slot count for a given machine and
/// the model it would run. Never returns 0 (a machine that can run the
/// model at all can run it at 1 slot) or more than `MAX_RECOMMENDED_SLOTS`.
pub fn compute_recommended_slots(ram_gb: f64, cpu_cores: usize, model_size_gb: f64) -> usize {
    let cpu_budget = (cpu_cores / CORES_PER_SLOT).max(1);
    max_safe_slots(ram_gb, model_size_gb).min(cpu_budget)
}

/// Highest-tier catalog model whose `min_ram_gb` fits within the machine's
/// RAM (minus the same OS reserve `compute_recommended_slots` uses). Reuses
/// the existing catalog data in `llm_manager::get_available_models` — no new
/// hardware-fit logic duplicated there.
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
        // 2026-07-30: the pipeline no longer gates on a separate 16GB tier
        // -- Layer6LlmLayer::run already checks for a real downloaded/
        // selected model itself. The pre-gate now only rules out hardware
        // below the catalog's smallest tier's floor (8GB, gemma4_e4b).
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

    /// audit_06 #4: the manual-override ceiling has to come from RAM, not from
    /// a flat 10. It must stay *above* the recommendation (a user overruling
    /// the CPU-based advice is legitimate) while still refusing a slot count
    /// the machine cannot hold — each slot costs a KV cache plus 2048 tokens
    /// of `--context-size`.
    #[test]
    fn max_safe_slots_bounds_the_override_by_ram_not_by_cores() {
        // 16GB, 8GB model: (16 - 8 - 4) / 1 = 4 slots of headroom. The flat
        // ceiling this replaced would have allowed 10 and OOM'd the server.
        assert_eq!(max_safe_slots(16.0, 8.0), 4);

        // Same machine, few cores: the *recommendation* drops to 1, but the
        // override ceiling stays at 4 — cores are a throughput opinion.
        assert_eq!(compute_recommended_slots(16.0, 2, 8.0), 1);
        assert!(max_safe_slots(16.0, 8.0) >= compute_recommended_slots(16.0, 2, 8.0));

        // Never 0, even when the model barely fits at all.
        assert_eq!(max_safe_slots(9.5, 5.0), 1);
        assert_eq!(max_safe_slots(8.0, 5.0), 1);

        // Still capped at the hard maximum on a huge machine.
        assert_eq!(max_safe_slots(256.0, 5.0), MAX_RECOMMENDED_SLOTS);
    }

    #[test]
    fn recommended_slots_clamps_to_1_when_ram_budget_negative() {
        // 8GB total - 5GB model - 4GB OS reserve = negative budget.
        assert_eq!(compute_recommended_slots(8.0, 8, 5.0), 1);
    }

    #[test]
    fn recommended_slots_clamps_on_cpu_budget_when_cores_low() {
        // RAM budget is huge (55 slots worth), but only 2 cores / 2 per slot = 1.
        assert_eq!(compute_recommended_slots(64.0, 2, 5.0), 1);
    }

    #[test]
    fn recommended_slots_clamps_at_max_10_when_both_budgets_are_high() {
        assert_eq!(compute_recommended_slots(256.0, 40, 5.0), 10);
    }

    #[test]
    fn recommended_slots_never_zero_at_the_ram_floor() {
        // 9.5 - 5 - 4 = 0.5, floors to 0 before the final max(1.0) clamp.
        assert_eq!(compute_recommended_slots(9.5, 8, 5.0), 1);
    }

    #[test]
    fn recommended_slots_realistic_mid_tier_machine() {
        // 24GB RAM, 10 cores, 5GB model: ram_budget=15, cpu_budget=5 -> 5.
        assert_eq!(compute_recommended_slots(24.0, 10, 5.0), 5);
    }

    #[test]
    fn recommend_model_id_picks_highest_fitting_tier() {
        // 40GB budget (36 after reserve) fits every catalog tier (max min_ram_gb is 32).
        assert_eq!(recommend_model_id(40.0), Some("gemma4_31b".to_string()));
    }

    #[test]
    fn recommend_model_id_picks_mid_tier_when_top_tiers_dont_fit() {
        // 20GB budget (16 after reserve): e4b(8), gemma4_12b(16), qwen3_6_27b(16) fit;
        // qwen3_6_35b_a3b(32) and gemma4_31b(32) don't. Highest tier among fitting is
        // qwen3_6_27b (tier 3), not gemma4_12b (tier 2).
        assert_eq!(recommend_model_id(20.0), Some("qwen3_6_27b".to_string()));
    }

    #[test]
    fn recommend_model_id_falls_back_to_smallest_tier() {
        // 12GB budget (8 after reserve): only gemma4_e4b (min_ram_gb 8.0) fits.
        assert_eq!(recommend_model_id(12.0), Some("gemma4_e4b".to_string()));
    }

    #[test]
    fn recommend_model_id_returns_none_below_every_tier_floor() {
        // 5GB budget (1 after reserve): nothing fits, must not panic.
        assert_eq!(recommend_model_id(5.0), None);
    }
}
