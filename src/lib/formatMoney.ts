/**
 * The one money formatter (audit Phase 2 §2.3, owner decision).
 *
 * Money is stored and passed around as an integer count of minor units (paise,
 * cents); converting to a decimal string is a presentation concern and happens
 * here at the last moment, with the row's own ISO currency code.
 *
 * A null amount is a real state -- an extraction that could not recover a
 * value -- and renders as an em dash rather than as zero.
 */

/** Debits read "−", credits "+": direction is never shown by colour alone (WCAG 1.4.1). */
export function formatMoney(
  amountMinor: number | null | undefined,
  currency: string | null | undefined,
  direction?: string | null
): string {
  if (amountMinor == null) return '—';
  const negative = direction === 'debit' || (direction !== 'credit' && amountMinor < 0);
  const sign = negative ? '−' : direction === 'credit' ? '+' : '';
  return `${sign}${formatAbs(Math.abs(amountMinor) / 100, currency || 'INR')}`;
}

/** Text colour for a direction: red for money out, green for money in. */
export function moneyTone(direction: string | null | undefined): string {
  return direction === 'debit' ? 'text-red-700' : direction === 'credit' ? 'text-emerald-700' : '';
}

/** Minor units from a major-unit figure, for the few IPC fields still sent as floats. */
export const toMinor = (major: number): number => Math.round(major * 100);

function formatAbs(major: number, currency: string): string {
  try {
    return new Intl.NumberFormat('en-IN', {
      style: 'currency',
      currency,
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    }).format(major);
  } catch {
    // Not a valid ISO code: show it as-is, so the amount is still unambiguous.
    const digits = major.toLocaleString('en-IN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
    return `${currency} ${digits}`;
  }
}
