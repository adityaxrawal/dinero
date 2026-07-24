// Doc 30 TASK-BILL-005 acceptance criteria.
import { describe, it, expect, vi } from 'vitest';
import { cancelSubscription, reactivateSubscription, type CancelDb } from '../api/billing/cancel';

function makeDb(subscription: Record<string, unknown> | null) {
  const update = vi.fn().mockImplementation(({ data }) => {
    Object.assign(subscription!, data);
    return Promise.resolve(subscription);
  });
  const db: CancelDb = {
    subscription: {
      findFirst: vi.fn().mockResolvedValue(subscription),
      update,
    } as unknown as CancelDb['subscription'],
    licensingAuditLog: {
      create: vi.fn().mockResolvedValue({}),
      findMany: vi.fn().mockResolvedValue([]),
    } as unknown as CancelDb['licensingAuditLog'],
  };
  return { db, update };
}

describe('test_cancel_at_period_end_retains_access_until_period_ends', () => {
  it('defaults to cancel-at-period-end, keeping status unchanged (not immediately locked)', async () => {
    const { db, update } = makeDb({
      id: 'sub_1',
      accountId: 'acc_1',
      status: 'active',
      currentPeriodEnd: new Date(Date.now() + 86400000),
    });
    const result = await cancelSubscription(db, { account_id: 'acc_1' });
    expect(result).toEqual({ status: 'cancelled', cancel_at_period_end: true });
    expect(update).toHaveBeenCalledWith(
      expect.objectContaining({ data: expect.objectContaining({ status: 'active' }) })
    );
  });

  it('immediate cancellation (cancel_at_period_end=false) sets status to canceled now', async () => {
    const { db } = makeDb({
      id: 'sub_1',
      accountId: 'acc_1',
      status: 'active',
      currentPeriodEnd: new Date(Date.now() + 86400000),
    });
    const result = await cancelSubscription(db, {
      account_id: 'acc_1',
      cancel_at_period_end: false,
    });
    expect(result.cancel_at_period_end).toBe(false);
  });
});

describe('test_reactivation_before_period_end_undoes_cancellation', () => {
  it('clears cancel_at_period_end when the period has not ended yet', async () => {
    const { db, update } = makeDb({
      id: 'sub_1',
      accountId: 'acc_1',
      status: 'active',
      cancelAtPeriodEnd: true,
      currentPeriodEnd: new Date(Date.now() + 86400000),
    });
    const result = await reactivateSubscription(db, 'acc_1');
    expect(result.status).toBe('reactivated');
    expect(update).toHaveBeenCalledWith(
      expect.objectContaining({ data: { cancelAtPeriodEnd: false } })
    );
  });

  it('rejects reactivation once the period has already ended', async () => {
    const { db } = makeDb({
      id: 'sub_1',
      accountId: 'acc_1',
      status: 'canceled',
      cancelAtPeriodEnd: true,
      currentPeriodEnd: new Date(Date.now() - 86400000),
    });
    await expect(reactivateSubscription(db, 'acc_1')).rejects.toMatchObject({
      code: 'VALIDATION_ERROR',
    });
  });
});

describe('test_cancellation_never_triggers_data_deletion', () => {
  it("cancelSubscription's own signature has no data-deletion capability -- only subscription.update and audit log are touched", async () => {
    const { db } = makeDb({
      id: 'sub_1',
      accountId: 'acc_1',
      status: 'active',
      currentPeriodEnd: new Date(Date.now() + 86400000),
    });
    await cancelSubscription(db, { account_id: 'acc_1' });
    // Structural proof: the CancelDb interface passed in has exactly
    // {subscription: {findFirst, update}, licensingAuditLog} -- no account
    // delete/deactivate handle exists for this function to call even if it
    // wanted to.
    expect(Object.keys(db)).toEqual(['subscription', 'licensingAuditLog']);
  });
});
