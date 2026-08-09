// Doc 43 §5 fix: this app's own quality-gate documentation (phase10's
// test_quality_gates_thresholds) never claims these are *measured in
// production* — they're target thresholds the test suite checks against
// fixed literals, not values derived from real usage data. Presented as
// declared targets, not live metrics, to avoid the same "fabricated live
// number" pattern already fixed elsewhere in this app (G9-G13).
export const QUALITY_GATE_TARGETS = [
  { label: 'Extraction Accuracy', target: '≥ 95%', nfr: 'NFR-003' },
  { label: 'False Positive Rate', target: '< 0.1%', nfr: 'NFR-004' },
  { label: 'False Merge Rate', target: '< 0.1%', nfr: 'NFR-005' },
];
