/**
 * Threshold targets each quality metric is judged against.
 *
 * Declared as data so the panel renders pass/fail from one place rather than
 * hard-coding comparisons per metric.
 */
export const QUALITY_GATE_TARGETS = [
  { label: 'Extraction Accuracy', target: '≥ 95%', nfr: 'NFR-003' },
  { label: 'False Positive Rate', target: '< 0.1%', nfr: 'NFR-004' },
  { label: 'False Merge Rate', target: '< 0.1%', nfr: 'NFR-005' },
];
