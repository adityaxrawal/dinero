import { describe, it, expect } from 'vitest';
import { getLicenseCta } from './licenseCta';

// Doc 30 TASK-BILL-010: test_billing_card_renders_correct_cta_per_state.
describe('getLicenseCta', () => {
  it('ACTIVE -> Manage Billing', () => {
    expect(getLicenseCta('ACTIVE', null)).toEqual({
      action: 'manage_billing',
      label: 'Manage Billing',
    });
  });

  it('GRACE -> Update Payment Method', () => {
    expect(getLicenseCta('GRACE', 3)).toEqual({
      action: 'update_payment_method',
      label: 'Update Payment Method',
    });
  });

  it('LOCKED -> Reactivate Subscription', () => {
    expect(getLicenseCta('LOCKED', null)).toEqual({
      action: 'reactivate',
      label: 'Reactivate Subscription',
    });
  });

  it('TRIAL -> Subscribe now with a countdown', () => {
    expect(getLicenseCta('TRIAL', 5)).toEqual({
      action: 'subscribe',
      label: 'Subscribe now (5d left)',
    });
  });

  it('ANONYMOUS_EVAL (or any other state) -> Subscribe now, no countdown if days unknown', () => {
    expect(getLicenseCta('ANONYMOUS_EVAL', null)).toEqual({
      action: 'subscribe',
      label: 'Subscribe now',
    });
  });
});
