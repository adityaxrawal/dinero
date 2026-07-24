// Doc 30 TASK-BILL-008 acceptance criteria.
import { describe, it, expect, vi } from 'vitest';
import { runBillingReconciliation, type ReconciliationDb } from '../jobs/billing_reconciliation';

function makeDb(
  subscriptions: { id: string; accountId: string; status: string }[],
  records: Record<string, string>
) {
  const update = vi.fn().mockResolvedValue({});
  const db: ReconciliationDb = {
    subscription: {
      findMany: vi.fn().mockResolvedValue(subscriptions),
      update,
    } as unknown as ReconciliationDb['subscription'],
    paymentProviderRecord: {
      findFirst: vi
        .fn()
        .mockImplementation(({ where }) =>
          Promise.resolve(
            records[where.subscriptionId]
              ? { razorpaySubscriptionId: records[where.subscriptionId] }
              : null
          )
        ),
    } as unknown as ReconciliationDb['paymentProviderRecord'],
    licensingAuditLog: {
      create: vi.fn().mockResolvedValue({}),
      findMany: vi.fn().mockResolvedValue([]),
    } as unknown as ReconciliationDb['licensingAuditLog'],
  };
  return { db, update };
}

describe('test_drift_detected_and_corrected', () => {
  it('corrects a subscription whose local status disagrees with Razorpay', async () => {
    const { db, update } = makeDb([{ id: 'sub_1', accountId: 'acc_1', status: 'active' }], {
      sub_1: 'rzp_sub_1',
    });
    const razorpay = { fetch: vi.fn().mockResolvedValue({ status: 'halted' }) }; // halted -> past_due
    const summary = await runBillingReconciliation(db, razorpay);
    expect(summary).toEqual({ checked: 1, drifted: 1, corrected: 1 });
    expect(update).toHaveBeenCalledWith(expect.objectContaining({ data: { status: 'past_due' } }));
  });

  it('does nothing when local status already matches Razorpay', async () => {
    const { db, update } = makeDb([{ id: 'sub_1', accountId: 'acc_1', status: 'active' }], {
      sub_1: 'rzp_sub_1',
    });
    const razorpay = { fetch: vi.fn().mockResolvedValue({ status: 'active' }) };
    const summary = await runBillingReconciliation(db, razorpay);
    expect(summary).toEqual({ checked: 1, drifted: 0, corrected: 0 });
    expect(update).not.toHaveBeenCalled();
  });
});

describe('test_correction_logged_to_audit_not_silent', () => {
  it('logs before/after status and the razorpay status observed', async () => {
    const { db } = makeDb([{ id: 'sub_1', accountId: 'acc_1', status: 'active' }], {
      sub_1: 'rzp_sub_1',
    });
    const razorpay = { fetch: vi.fn().mockResolvedValue({ status: 'cancelled' }) };
    await runBillingReconciliation(db, razorpay);
    expect(db.licensingAuditLog.create).toHaveBeenCalledWith(
      expect.objectContaining({
        data: expect.objectContaining({
          eventType: 'billing_reconciliation_correction',
          payload: expect.objectContaining({
            local_status_was: 'active',
            corrected_to: 'canceled',
          }),
        }),
      })
    );
  });
});

describe('test_reconciliation_summary_report_generated', () => {
  it('reports checked/drifted/corrected counts across multiple subscriptions', async () => {
    const { db } = makeDb(
      [
        { id: 'sub_1', accountId: 'acc_1', status: 'active' },
        { id: 'sub_2', accountId: 'acc_2', status: 'active' },
      ],
      { sub_1: 'rzp_sub_1', sub_2: 'rzp_sub_2' }
    );
    const razorpay = {
      fetch: vi
        .fn()
        .mockImplementation((id) =>
          Promise.resolve({ status: id === 'rzp_sub_1' ? 'halted' : 'active' })
        ),
    };
    const summary = await runBillingReconciliation(db, razorpay);
    expect(summary).toEqual({ checked: 2, drifted: 1, corrected: 1 });
  });
});
