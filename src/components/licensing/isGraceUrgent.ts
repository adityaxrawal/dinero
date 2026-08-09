/**
 * Doc 30 TASK-BILL-004: the grace banner escalates from informational amber
 * (day 1-3 of the 7-day grace window) to a prominent red state with a direct
 * "Update Payment Method" deep link (day 4-7). `daysRemainingInTrial` (the
 * field name is reused from the trial countdown, same underlying
 * `days_remaining` response field, Document 19 §14.1) counts down from 7, so
 * <=3 remaining means >=4 days have already elapsed.
 */
const GRACE_URGENT_THRESHOLD_DAYS_REMAINING = 3;

export function isGraceUrgent(daysRemaining: number | null): boolean {
  return daysRemaining !== null && daysRemaining <= GRACE_URGENT_THRESHOLD_DAYS_REMAINING;
}
