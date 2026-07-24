/** Formats a foreign-currency minor-unit amount (e.g. original_amount_minor). */
export function formatMoney(amountMinor: number | null, currency: string | null): string {
  if (amountMinor === null) return '—';
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
  return `${symbol}${(amountMinor / 100).toLocaleString(undefined, { minimumFractionDigits: 2 })}`;
}
