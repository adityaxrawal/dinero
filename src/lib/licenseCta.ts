/**
 * Chooses the call-to-action shown for the user's current subscription state.
 *
 * Keeping this mapping in one pure function means every surface that offers a
 * billing action -- banner, settings screen, lock screen -- derives its button
 * from the same rule, so the wording and the resulting action can never drift
 * apart between them.
 */

/** The billing flow a CTA should trigger when activated. */
export type LicenseCtaAction =
  | 'subscribe'
  | 'manage_billing'
  | 'update_payment_method'
  | 'reactivate';

/** A resolved CTA: the action to run, paired with the label to display. */
export interface LicenseCta {
  action: LicenseCtaAction;
  label: string;
}

/**
 * Map a license state to the action that state should offer.
 *
 * `daysRemaining` only affects the trial case, where the countdown is folded
 * into the label to give the prompt urgency; the other states have no
 * time-sensitive component.
 */
export function getLicenseCta(state: string, daysRemaining: number | null): LicenseCta {
  switch (state) {
    // Paid and current: nothing to sell, so the action is account management.
    case 'ACTIVE':
      return { action: 'manage_billing', label: 'Manage Billing' };

    // Payment failed but access continues for now -- the fix is a new payment
    // method, not a fresh purchase.
    case 'GRACE':
      return { action: 'update_payment_method', label: 'Update Payment Method' };

    // Grace has elapsed and access is withdrawn; the subscription still exists
    // and can be revived rather than bought again.
    case 'LOCKED':
      return { action: 'reactivate', label: 'Reactivate Subscription' };

    // Trial and any unrecognised state fall through to the acquisition path,
    // which is the safe default -- an unknown state should never present itself
    // as though the user already has a working subscription.
    default:
      return {
        action: 'subscribe',
        label: daysRemaining != null ? `Subscribe now (${daysRemaining}d left)` : 'Subscribe now',
      };
  }
}
