/**
 * Decides when grace-period messaging should escalate in urgency.
 */
const GRACE_URGENT_THRESHOLD_DAYS_REMAINING = 3;

/** Whether grace-period messaging should escalate in urgency. */
export function isGraceUrgent(daysRemaining: number | null): boolean {
  return daysRemaining !== null && daysRemaining <= GRACE_URGENT_THRESHOLD_DAYS_REMAINING;
}
