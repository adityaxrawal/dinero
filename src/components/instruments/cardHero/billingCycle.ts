/**
 * Derives the current billing cycle and days remaining for an instrument.
 *
 * Billing cycles are expressed as a day-of-month, so the countdown has to roll
 * over into the following month once that day has passed.
 */

/**
 * Days until the next billing date, given a day-of-month cycle day.
 *
 * A negative difference means the cycle day has already passed this month, so
 * the remaining days of the current month are added to carry into the next one.
 * The month length is read from a real date rather than assumed, which keeps
 * this correct for short months and February in a leap year.
 */
function daysUntilBilling(cycleDay: number, today = new Date()): number {
  const daysLeft = cycleDay - today.getDate();
  if (daysLeft >= 0) return daysLeft;
  const daysInMonth = new Date(today.getFullYear(), today.getMonth() + 1, 0).getDate();
  return daysLeft + daysInMonth;
}

/**
 * Human-readable billing-cycle text for an instrument's card hero.
 *
 * Wording adapts to instrument type, since a statement cycle only applies to
 * accounts that actually have one.
 */
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
