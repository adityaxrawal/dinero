/** Extracted for TASK-FE-009/010 reuse (was inline in the old Transactions.tsx). */
export function formatCustomDate(dateString: string): string {
  const d = new Date(dateString);
  const day = d.getDate();
  const getOrdinal = (n: number) => {
    const s = ['th', 'st', 'nd', 'rd'];
    const v = n % 100;
    return n + (s[(v - 20) % 10] || s[v] || s[0]);
  };
  const month = d.toLocaleString('en-US', { month: 'short' });
  const year = d.getFullYear().toString().slice(-2);
  return `${getOrdinal(day)} ${month} ${year}'`;
}
