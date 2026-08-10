//! Measures extraction accuracy against the synthetic benchmark corpus.
//!
//! Tracks which ladder layer contributed each field, which is what makes the
//! cost/accuracy trade-off visible: a change that improves accuracy by pushing
//! everything to the LLM layer is a regression in the terms that matter here.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const ACCURACY_GATE_THRESHOLD_PCT: f64 = 95.0;
pub const FALSE_POSITIVE_GATE_THRESHOLD_PCT: f64 = 0.1;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LayerStats {
    pub total: u64,
    pub correct: u64,
}

impl LayerStats {
    /// Accuracy as a percentage of fields correctly extracted.
    pub fn accuracy_pct(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.correct as f64 / self.total as f64) * 100.0
        }
    }
}

#[derive(Debug, Default)]
pub struct LayerContributionTracker {
    per_layer: BTreeMap<String, LayerStats>,
}

impl LayerContributionTracker {
    /// An empty tracker with no layer recorded yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one field outcome against the layer that produced it.
    ///
    /// Attributing results per layer is what makes the cost/accuracy trade-off
    /// visible -- accuracy bought by escalating everything to the LLM is a regression
    /// in the terms that matter here, not an improvement.
    pub fn record(&mut self, extraction_method: Option<&str>, field_correct: bool) {
        let key = extraction_method.unwrap_or("no_extraction").to_string();
        let entry = self.per_layer.entry(key).or_default();
        entry.total += 1;
        if field_correct {
            entry.correct += 1;
        }
    }

    /// Consumes the tracker into per-layer statistics.
    ///
    /// A BTreeMap so layers report in a stable order and successive runs are directly
    /// comparable.
    pub fn into_breakdown(self) -> BTreeMap<String, LayerStats> {
        self.per_layer
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricResult {
    pub value_pct: f64,
    pub threshold_pct: f64,
    pub gate_passed: bool,
    pub sample_count: usize,
}

impl MetricResult {
    /// Builds a metric result from its counts.
    pub fn new(
        value_pct: f64,
        threshold_pct: f64,
        sample_count: usize,
        higher_is_better: bool,
    ) -> Self {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub metric: String,
    pub generated_at: String,
    pub result: MetricResult,
    pub per_layer_contribution: BTreeMap<String, LayerStats>,
}

impl BenchmarkReport {
    /// Assembles the full benchmark report from its metrics and layer breakdown.
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

    /// Serialises the report for storage and diffing between runs.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Writes the report to disk.
    pub fn write_to_path(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = self.to_json_pretty().map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }
}

/// Path of the report file for a benchmark metric.
pub fn report_path(metric: &str) -> std::path::PathBuf {
    let dir =
        std::env::var("BENCHMARK_REPORT_DIR").unwrap_or_else(|_| "target/benchmark".to_string());
    std::path::PathBuf::from(dir).join(format!("{}_report.json", metric))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer_stats_accuracy_pct() {
        let stats = LayerStats {
            total: 4,
            correct: 3,
        };
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
        assert_eq!(
            breakdown["bank_templates"],
            LayerStats {
                total: 2,
                correct: 1
            }
        );
        assert_eq!(
            breakdown["nlp"],
            LayerStats {
                total: 1,
                correct: 1
            }
        );
        assert_eq!(
            breakdown["no_extraction"],
            LayerStats {
                total: 1,
                correct: 0
            }
        );
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
        let tmp =
            std::env::temp_dir().join(format!("dinero_benchmark_test_{}", uuid::Uuid::new_v4()));
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
