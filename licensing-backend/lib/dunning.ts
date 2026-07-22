// Doc 30 TASK-BILL-004: "relies on Razorpay's own automatic retry schedule
// for failed recurring charges (typically 3 retries over ~7 days) rather
// than custom retry logic -- the backend's job is simply to reflect
// accurate status on each validate call." There is deliberately no retry
// scheduling code here -- webhooks/razorpay.ts's invoice.payment_failed/
// invoice.paid handlers already are that "reflect accurate status" job.
// This module exists to make that non-decision explicit and documented,
// not to add behavior.

/// Whether a subscription in `past_due` should still be treated as usable
/// server-side (it should -- the 7-day offline grace that actually gates
/// desktop access is computed entirely client-side, TASK-AUTH-009). This
/// backend never independently locks anything based on elapsed dunning time.
export function isPastDueStillServerSideUsable(status: string): boolean {
  return status === 'past_due' || status === 'active' || status === 'trialing';
}
