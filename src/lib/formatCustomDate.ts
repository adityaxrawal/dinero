/**
 * Renders a date in the app's compact house style, e.g. "3rd Feb 26'".
 *
 * Deliberately not `toLocaleDateString` -- the ordinal day suffix and the
 * trailing-apostrophe two-digit year are a bespoke presentation format that no
 * locale produces, and it must render identically on every machine.
 */
export function formatCustomDate(dateString: string): string {
  const d = new Date(dateString);
  const day = d.getDate();

  // English ordinal suffix. The `% 100` and the `(v - 20) % 10` step are what
  // keep 11th/12th/13th correct -- those take "th" despite ending in 1/2/3,
  // whereas 21st/22nd/23rd follow the ordinary pattern. Out-of-range indices
  // fall through to 'th' via the chained `||`.
  const getOrdinal = (n: number) => {
    const s = ['th', 'st', 'nd', 'rd'];
    const v = n % 100;
    return n + (s[(v - 20) % 10] || s[v] || s[0]);
  };

  // Month name is pinned to en-US so the abbreviation never shifts with the
  // host locale, keeping the format stable across machines.
  const month = d.toLocaleString('en-US', { month: 'short' });
  const year = d.getFullYear().toString().slice(-2);
  return `${getOrdinal(day)} ${month} ${year}'`;
}
