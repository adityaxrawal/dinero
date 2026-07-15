// Doc 30 TASK-TXN-014: Extraction Accuracy Benchmark Harness.
//
// The real corpus-loading / field-comparison logic already lived in
// `phase10_quality_gates_tests.rs` (test_nfr_003/004/005) before this task
// and is correct -- it runs the real 6-layer extraction ladder against the
// real Document 34 §5 corpus and enforces the Document 34 §10.2 thresholds.
// What was missing, per Doc 30's task text, is (a) a per-layer contribution
// breakdown (which of the 6 layers produced each correct/incorrect field)
// and (b) a structured JSON report a CI job can gate on and diff across
// runs. This module is that reusable, corpus-independent reporting layer;
// `phase10_quality_gates_tests.rs` feeds it real per-sample results.
//
// Doc 30 also names `src-tauri/tests/benchmark_corpus.rs` as a file for
// this task. The existing corpus tests were deliberately left in place in
// `phase10_quality_gates_tests.rs` rather than relocated into a new
// integration-test binary: they already run correctly, are already wired
// into `.github/workflows/benchmark.yml` via `cargo test
// phase10_quality_gates`, and moving a working, CI-wired test for no
// functional gain would be exactly the kind of gratuitous churn the master
// prompt's principles warn against. Effort went into the actually-missing
// capability instead.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Document 34 §10.2 quality-gate thresholds.
pub const ACCURACY_GATE_THRESHOLD_PCT: f64 = 95.0;
pub const FALSE_POSITIVE_GATE_THRESHOLD_PCT: f64 = 0.1;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LayerStats {
    pub total: u64,
    pub correct: u64,
}

impl LayerStats {
    pub fn accuracy_pct(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.correct as f64 / self.total as f64) * 100.0
        }
    }
}

/// Accumulates per-extraction-layer correct/total field counts as a
/// benchmark run compares each sample's actual result against its
/// ground-truth label. Keyed by `ExtractionResult::extraction_method`
/// (e.g. "learned_patterns", "bank_templates", "generic_regex", "nlp",
/// "layer5_statement_crossref", "llm_layer6") plus a synthetic
/// `"no_extraction"` bucket for samples where every layer returned nothing.
#[derive(Debug, Default)]
pub struct LayerContributionTracker {
    per_layer: BTreeMap<String, LayerStats>,
}

impl LayerContributionTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, extraction_method: Option<&str>, field_correct: bool) {
        let key = extraction_method.unwrap_or("no_extraction").to_string();
        let entry = self.per_layer.entry(key).or_default();
        entry.total += 1;
        if field_correct {
            entry.correct += 1;
        }
    }

    pub fn into_breakdown(self) -> BTreeMap<String, LayerStats> {
        self.per_layer
    }
}

/// A single quality-gate metric (extraction accuracy, or false-positive
/// rate) as reported for one benchmark run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricResult {
    pub value_pct: f64,
    pub threshold_pct: f64,
    pub gate_passed: bool,
    pub sample_count: usize,
}

impl MetricResult {
    pub fn new(value_pct: f64, threshold_pct: f64, sample_count: usize, higher_is_better: bool) -> Self {
        let gate_passed = if higher_is_better {
            value_pct >= threshold_pct
        } else {
            value_pct <= threshold_pct
        };
        Self {
            value_pct,
            threshold_pct,
            gate_passed,
            sample_count,
        }
    }
}

/// Structured JSON report for one benchmark metric run (Doc 30 TASK-TXN-014:
/// "output a structured JSON report the CI job can gate on; track results
/// over time"). Kept scoped to a single metric per report file, since the
/// underlying corpus tests execute as separate, independently-parallelized
/// `cargo test` cases -- one shared mutable file across concurrent test
/// threads would race. `.github/workflows/benchmark.yml` uploads each
/// report as a CI artifact, which is what gives "track results over time":
/// GitHub retains one artifact set per run, so historical comparison is a
/// matter of diffing artifacts across nightly runs, not something this
/// process needs to maintain state for itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub metric: String,
    pub generated_at: String,
    pub result: MetricResult,
    pub per_layer_contribution: BTreeMap<String, LayerStats>,
}

impl BenchmarkReport {
    pub fn new(
        metric: &str,
        result: MetricResult,
        per_layer_contribution: BTreeMap<String, LayerStats>,
    ) -> Self {
        Self {
            metric: metric.to_string(),
            generated_at: chrono::Utc::now().to_rfc3339(),
            result,
            per_layer_contribution,
        }
    }

    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Best-effort write. Report generation must never flip a passing test
    /// to a failure just because the filesystem is read-only or the target
    /// directory can't be created in some CI sandbox -- the gate is the
    /// `assert!` on `result.gate_passed` still living in the test itself;
    /// this is a side artifact, not the pass/fail mechanism.
    pub fn write_to_path(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = self
            .to_json_pretty()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json)
    }
}

/// Where to write a benchmark report for the given metric name. Honors
/// `BENCHMARK_REPORT_DIR` (set by `.github/workflows/benchmark.yml`) so CI
/// can control the artifact-upload location; falls back to
/// `target/benchmark/` for local runs.
pub fn report_path(metric: &str) -> std::path::PathBuf {
    let dir = std::env::var("BENCHMARK_REPORT_DIR").unwrap_or_else(|_| "target/benchmark".to_string());
    std::path::PathBuf::from(dir).join(format!("{}_report.json", metric))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer_stats_accuracy_pct() {
        let stats = LayerStats { total: 4, correct: 3 };
        assert_eq!(stats.accuracy_pct(), 75.0);
    }

    #[test]
    fn test_layer_stats_accuracy_pct_zero_total() {
        let stats = LayerStats::default();
        assert_eq!(stats.accuracy_pct(), 0.0);
    }

    #[test]
    fn test_tracker_buckets_by_layer() {
        let mut tracker = LayerContributionTracker::new();
        tracker.record(Some("bank_templates"), true);
        tracker.record(Some("bank_templates"), false);
        tracker.record(Some("nlp"), true);
        tracker.record(None, false);

        let breakdown = tracker.into_breakdown();
        assert_eq!(breakdown["bank_templates"], LayerStats { total: 2, correct: 1 });
        assert_eq!(breakdown["nlp"], LayerStats { total: 1, correct: 1 });
        assert_eq!(breakdown["no_extraction"], LayerStats { total: 1, correct: 0 });
    }

    #[test]
    fn test_metric_result_higher_is_better_gate() {
        let passing = MetricResult::new(96.0, ACCURACY_GATE_THRESHOLD_PCT, 100, true);
        assert!(passing.gate_passed);
        let failing = MetricResult::new(94.0, ACCURACY_GATE_THRESHOLD_PCT, 100, true);
        assert!(!failing.gate_passed);
    }

    #[test]
    fn test_metric_result_lower_is_better_gate() {
        let passing = MetricResult::new(0.05, FALSE_POSITIVE_GATE_THRESHOLD_PCT, 100, false);
        assert!(passing.gate_passed);
        let failing = MetricResult::new(0.2, FALSE_POSITIVE_GATE_THRESHOLD_PCT, 100, false);
        assert!(!failing.gate_passed);
    }

    #[test]
    fn test_report_round_trips_through_json() {
        let mut tracker = LayerContributionTracker::new();
        tracker.record(Some("learned_patterns"), true);
        let report = BenchmarkReport::new(
            "extraction_accuracy",
            MetricResult::new(100.0, ACCURACY_GATE_THRESHOLD_PCT, 1, true),
            tracker.into_breakdown(),
        );
        let json = report.to_json_pretty().unwrap();
        let parsed: BenchmarkReport = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.metric, "extraction_accuracy");
        assert!(parsed.result.gate_passed);
        assert_eq!(parsed.per_layer_contribution["learned_patterns"].correct, 1);
    }

    #[test]
    fn test_write_to_path_creates_parent_dirs() {
        let tmp = std::env::temp_dir().join(format!("dinero_benchmark_test_{}", uuid::Uuid::new_v4()));
        let report = BenchmarkReport::new(
            "false_positive_rate",
            MetricResult::new(0.0, FALSE_POSITIVE_GATE_THRESHOLD_PCT, 10, false),
            BTreeMap::new(),
        );
        let path = tmp.join("nested").join("report.json");
        report.write_to_path(&path).unwrap();
        assert!(path.exists());
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("false_positive_rate"));
        std::fs::remove_dir_all(&tmp).ok();
    }
}
