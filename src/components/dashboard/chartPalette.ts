/**
 * TASK-FE-008: categorical + sequential chart palette. Fixed slot order is
 * the CVD-safety mechanism (never cycled) — validated via the dataviz
 * skill's `validate_palette.js` (worst adjacent CVD ΔE 24.2, well clear of
 * the >=12 target; 3 slots fall below 3:1 contrast on a light surface, so
 * those slices always get a visible direct label, never color alone).
 * Dark-mode chart tokens are deliberately not defined — Document 14 §4.2:
 * dark mode activation is deferred to Phase 3, this app is light-mode only.
 */
export const CATEGORICAL_PALETTE = [
  '#2a78d6', // blue
  '#1baf7a', // aqua
  '#eda100', // yellow
  '#008300', // green
  '#4a3aa7', // violet
  '#e34948', // red
  '#e87ba4', // magenta
  '#eb6834', // orange
] as const;

// A 9th+ category never gets a generated hue — it folds into "Other" (see
// CategoryBreakdownChart's grouping logic) and takes this neutral gray.
export const OTHER_SLICE_COLOR = '#9ca3af';

// Sequential single hue (blue, mid step) for the one-series spend trend line.
export const SEQUENTIAL_LINE_COLOR = '#2a78d6';
