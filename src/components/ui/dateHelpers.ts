/**
 * Date helpers extracted from `date-picker.tsx` so that file exports only
 * components (react-refresh/only-export-components) — same convention as
 * `classifyBillUrgency.ts` / `groupCategoriesForChart.ts`.
 */

/** Parse YYYY-MM-DD into a local Date without a UTC shift. */
export function parseISODate(dateStr?: string | null): Date | null {
  if (!dateStr) return null;
  const parts = dateStr.slice(0, 10).split('-').map(Number);
  if (parts.length < 3 || parts.some(isNaN)) return null;
  const [year, month, day] = parts;
  return new Date(year, month - 1, day);
}

/** Format a Date into a YYYY-MM-DD local string. */
export function toISODate(date: Date): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, '0');
  const d = String(date.getDate()).padStart(2, '0');
  return `${y}-${m}-${d}`;
}

/** Human display format, e.g. 26 Jul 2026. */
export function formatDisplayDate(dateStr?: string | null): string {
  const parsed = parseISODate(dateStr);
  if (!parsed) return '';
  return parsed.toLocaleDateString('en-GB', {
    day: '2-digit',
    month: 'short',
    year: 'numeric',
  });
}
