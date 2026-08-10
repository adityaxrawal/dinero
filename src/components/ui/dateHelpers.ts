
/**
 * ISO date parsing and formatting for the pickers.
 *
 * Dates are handled as plain YYYY-MM-DD strings rather than Date objects
 * wherever possible, which sidesteps timezone shifts -- a Date constructed from
 * a bare date string is UTC midnight, and rendering it locally can land on the
 * previous day west of Greenwich.
 */
export function parseISODate(dateStr?: string | null): Date | null {
  if (!dateStr) return null;
  const parts = dateStr.slice(0, 10).split('-').map(Number);
  if (parts.length < 3 || parts.some(isNaN)) return null;
  const [year, month, day] = parts;
  return new Date(year, month - 1, day);
}

/**
 * Formats a Date as YYYY-MM-DD using its local components.
 *
 * Deliberately not toISOString, which converts to UTC and can shift the date by
 * a day for users west of Greenwich.
 */
export function toISODate(date: Date): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, '0');
  const d = String(date.getDate()).padStart(2, '0');
  return `${y}-${m}-${d}`;
}

/** Formats a date for display in the picker trigger. */
export function formatDisplayDate(dateStr?: string | null): string {
  const parsed = parseISODate(dateStr);
  if (!parsed) return '';
  return parsed.toLocaleDateString('en-GB', {
    day: '2-digit',
    month: 'short',
    year: 'numeric',
  });
}
