// Doc 30 TASK-BILL-008: the Vercel Cron entrypoint. Vercel signs cron
// requests with a bearer token matching CRON_SECRET; without that check any
// caller on the internet could drive a full billing reconciliation against
// the live Razorpay account.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import type { VercelRequest, VercelResponse } from '@vercel/node';

const runBillingReconciliation = vi.fn();
const getRazorpayCredentials = vi.fn();
const realRazorpaySubscriptions = vi.fn();

vi.mock('../lib/db', () => ({ prisma: {} }));
vi.mock('../jobs/billing_reconciliation', () => ({
  runBillingReconciliation: (...a: unknown[]) => runBillingReconciliation(...a),
}));
vi.mock('../lib/razorpay', () => ({
  getRazorpayCredentials: () => getRazorpayCredentials(),
  realRazorpaySubscriptions: (...a: unknown[]) => realRazorpaySubscriptions(...a),
}));

import handler from '../api/cron/billing-reconciliation';

function makeRes() {
  const res = {
    statusCode: 0,
    body: undefined as unknown,
    status(code: number) {
      this.statusCode = code;
      return this;
    },
    json(payload: unknown) {
      this.body = payload;
      return this;
    },
    setHeader: vi.fn(),
    end: vi.fn(),
  };
  return res as unknown as VercelResponse & { statusCode: number; body: unknown };
}

const req = (authorization?: string) =>
  ({ headers: authorization ? { authorization } : {}, method: 'POST' }) as unknown as VercelRequest;

const ORIGINAL_SECRET = process.env.CRON_SECRET;

beforeEach(() => {
  vi.clearAllMocks();
  process.env.CRON_SECRET = 'topsecret';
  getRazorpayCredentials.mockReturnValue({ keyId: 'key', keySecret: 'secret' });
  realRazorpaySubscriptions.mockReturnValue({});
  runBillingReconciliation.mockResolvedValue({ checked: 4, repaired: 1 });
});

afterEach(() => {
  if (ORIGINAL_SECRET === undefined) delete process.env.CRON_SECRET;
  else process.env.CRON_SECRET = ORIGINAL_SECRET;
});

describe('billing-reconciliation cron entrypoint', () => {
  it('rejects a request with no bearer token', async () => {
    const res = makeRes();
    await handler(req(), res);

    expect(res.statusCode).toBe(401);
    expect(runBillingReconciliation).not.toHaveBeenCalled();
  });

  it('rejects a request whose bearer token does not match CRON_SECRET', async () => {
    const res = makeRes();
    await handler(req('Bearer guessed'), res);

    expect(res.statusCode).toBe(401);
    expect(runBillingReconciliation).not.toHaveBeenCalled();
  });

  it('runs the reconciliation for a correctly signed cron request', async () => {
    const res = makeRes();
    await handler(req('Bearer topsecret'), res);

    expect(res.statusCode).toBe(200);
    expect(res.body).toEqual({ checked: 4, repaired: 1 });
    expect(runBillingReconciliation).toHaveBeenCalledTimes(1);
  });

  it('refuses to run when Razorpay credentials are missing', async () => {
    getRazorpayCredentials.mockReturnValue(null);
    const res = makeRes();
    await handler(req('Bearer topsecret'), res);

    expect(res.statusCode).toBe(500);
    expect(runBillingReconciliation).not.toHaveBeenCalled();
  });

  it('runs unauthenticated only when no CRON_SECRET is configured', async () => {
    // Documents the deliberate local/preview escape hatch: the guard is
    // skipped entirely when the env var is unset, so an unset CRON_SECRET in
    // production would leave this endpoint open.
    delete process.env.CRON_SECRET;
    const res = makeRes();
    await handler(req(), res);

    expect(res.statusCode).toBe(200);
    expect(runBillingReconciliation).toHaveBeenCalledTimes(1);
  });
});
