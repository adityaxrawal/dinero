/**
 * Formats a monetary amount for display, given a value in minor units.
 *
 * Money is stored and passed around as an integer count of minor units (paise,
 * cents) rather than a float, which is what keeps arithmetic exact; converting
 * to a decimal string is therefore strictly a presentation concern and happens
 * here at the last possible moment.
 *
 * A null amount is a real state in this app -- an extraction that could not
 * recover a value -- and renders as an em dash rather than as zero, since those
 * two mean very different things to the reader.
 */
export function formatMoney(amountMinor: number | null, currency: string | null): string {
  if (amountMinor === null) return '—';

  // Known currencies get their symbol; anything else falls back to the code
  // followed by a space, so an unrecognised currency is still unambiguous.
  // A missing currency defaults to rupees, this app's primary market.
  const symbol =
    currency === 'USD'
      ? '$'
      : currency === 'EUR'
        ? '€'
        : currency === 'GBP'
          ? '£'
          : currency
            ? `${currency} `
            : '₹';
  // Divide back to major units and force two decimal places, so amounts line
  // up in columns and a round figure still reads as money rather than a count.
  return `${symbol}${(amountMinor / 100).toLocaleString(undefined, { minimumFractionDigits: 2 })}`;
}
