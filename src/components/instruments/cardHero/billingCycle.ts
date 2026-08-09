/**
 * Days until the next statement is cut. A cycle day already past this month
 * rolls to the same day next month, so the count is never negative.
 */
function daysUntilBilling(cycleDay: number, today = new Date()): number {
  const daysLeft = cycleDay - today.getDate();
  if (daysLeft >= 0) return daysLeft;
  const daysInMonth = new Date(today.getFullYear(), today.getMonth() + 1, 0).getDate();
  return daysLeft + daysInMonth;
}

export function billingCycleText(
  instrumentType: string,
  cycleDay: number | null | undefined,
  today = new Date()
): string | null {
  if (instrumentType !== 'credit_card' || !cycleDay) return null;
  const daysLeft = daysUntilBilling(cycleDay, today);
  if (daysLeft === 0) return 'Bill generated today';
  return `Bill in ${daysLeft} ${daysLeft === 1 ? 'day' : 'days'}`;
}
