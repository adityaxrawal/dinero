// Doc 30 TASK-BILL-010: "state-appropriate CTA" mapping, extracted as a pure
// function so Settings.tsx's License & Billing section stays testable
// without mounting the entire (large) Settings page.
export type LicenseCtaAction =
  | 'subscribe'
  | 'manage_billing'
  | 'update_payment_method'
  | 'reactivate';

export interface LicenseCta {
  action: LicenseCtaAction;
  label: string;
}

export function getLicenseCta(state: string, daysRemaining: number | null): LicenseCta {
  switch (state) {
    case 'ACTIVE':
      return { action: 'manage_billing', label: 'Manage Billing' };
    case 'GRACE':
      return { action: 'update_payment_method', label: 'Update Payment Method' };
    case 'LOCKED':
      return { action: 'reactivate', label: 'Reactivate Subscription' };
    default:
      return {
        action: 'subscribe',
        label: daysRemaining != null ? `Subscribe now (${daysRemaining}d left)` : 'Subscribe now',
      };
  }
}
