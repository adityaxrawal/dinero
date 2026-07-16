/**
 * Guards the divide-by-zero case (`total` is 0 until the backend's first
 * `scan_progress` event reports a real message count). Kept in its own
 * module (not co-located in `HistoricalScanScreen.tsx`) so the component
 * file only exports the component — mixing a component default-export with
 * a named pure-function export in the same file breaks Fast Refresh's
 * component-boundary detection (`react-refresh/only-export-components`).
 */
export function scanProgressPercent(processed: number, total: number): number {
  if (total <= 0) return 0;
  return Math.round((processed / total) * 100);
}
