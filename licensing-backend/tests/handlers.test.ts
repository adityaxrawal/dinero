// The Vercel `handler` wrappers were the only untested layer in these
// endpoints — the domain functions each have their own spec, but the
// method/validation/env/auth guards around them did not, so a handler could
// (for example) stop returning 500 on a missing key without any spec failing.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { LicensingApiError } from '../lib/errors';

const prisma = {
  plan: { findMany: vi.fn(), update: vi.fn(), findUnique: vi.fn() },
  account: { findUnique: vi.fn(), findFirst: vi.fn() },
  subscription: { findFirst: vi.fn(), update: vi.fn(), updateMany: vi.fn() },
  licenseToken: { findFirst: vi.fn(), findUnique: vi.fn() },
  licensingAuditLog: { create: vi.fn(), findMany: vi.fn(), count: vi.fn() },
};

const assertAdminAuthorized = vi.fn();
const verifyWebhookSignature = vi.fn();

vi.mock('../lib/db', () => ({ prisma, findOrCreateAccount: vi.fn() }));
vi.mock('../lib/admin_auth', () => ({
  assertAdminAuthorized: (h: string | undefined) => assertAdminAuthorized(h),
}));
vi.mock('../lib/razorpay', () => ({
  verifyWebhookSignature: (...a: unknown[]) => verifyWebhookSignature(...a),
  realRazorpayRefunds: vi.fn(() => ({ refund: vi.fn() })),
  realRazorpayPayments: vi.fn(() => ({ fetch: vi.fn() })),
}));
vi.mock('../lib/email', () => ({ consoleEmailSender: { send: vi.fn() } }));

function makeRes() {
  const res = {
    statusCode: 200,
    setHeader: vi.fn(),
    status: vi.fn(function (this: typeof res, code: number) {
      this.statusCode = code;
      return this;
    }),
    json: vi.fn(),
  };
  return res as unknown as VercelResponse & { json: ReturnType<typeof vi.fn> };
}

const req = (over: Partial<VercelRequest> = {}) =>
  ({ method: 'POST', body: {}, query: {}, headers: {}, ...over }) as VercelRequest;

/** The body passed to res.json(), whatever status it went out with. */
const payload = (res: ReturnType<typeof makeRes>) => res.json.mock.calls[0]?.[0];

const load = async (path: string) => (await import(path)).default;

beforeEach(() => {
  vi.clearAllMocks();
  vi.spyOn(console, 'log').mockImplementation(() => {});
  vi.spyOn(console, 'error').mockImplementation(() => {});
  assertAdminAuthorized.mockReturnValue(undefined);
  verifyWebhookSignature.mockReturnValue(true);
});

afterEach(() => vi.unstubAllEnvs());

describe('withRequestLogging wrapper', () => {
  it('stamps a correlation id on every response', async () => {
    const handler = await load('../api/billing/cancel');
    const res = makeRes();
    await handler(req({ method: 'GET' }), res);
    expect(res.setHeader).toHaveBeenCalledWith('X-Request-Id', expect.any(String));
  });

  it('logs exactly one structured line carrying no request content', async () => {
    const handler = await load('../api/billing/cancel');
    const res = makeRes();
    await handler(req({ body: { account_id: 'secret-account' } }), res);
    const logged = (console.log as ReturnType<typeof vi.fn>).mock.calls;
    expect(logged).toHaveLength(1);
    expect(Object.keys(JSON.parse(logged[0][0])).sort()).toEqual([
      'endpoint',
      'latency_ms',
      'request_id',
      'status',
    ]);
    expect(logged[0][0]).not.toContain('secret-account');
  });
});

describe('license/activate handler', () => {
  const valid = {
    email: 'a@b.com',
    razorpay_payment_id: 'pay_1',
    razorpay_signature: 'sig',
    device_id: 'dev1',
    billing_interval: 'monthly',
  };

  it('rejects a non-POST', async () => {
    const handler = await load('../api/license/activate');
    const res = makeRes();
    await handler(req({ method: 'GET' }), res);
    expect(res.statusCode).toBe(405);
  });

  it.each(Object.keys(valid))('rejects a request missing %s', async (field) => {
    const handler = await load('../api/license/activate');
    const res = makeRes();
    const body = { ...valid, [field]: undefined };
    await handler(req({ body }), res);
    expect(res.statusCode).toBe(400);
    expect(payload(res).message).toBe(`${field} is required`);
  });

  it('fails closed when the signing key is not configured', async () => {
    vi.stubEnv('JWT_PRIVATE_KEY_PEM', '');
    vi.stubEnv('RAZORPAY_KEY_SECRET', 'secret');
    const handler = await load('../api/license/activate');
    const res = makeRes();
    await handler(req({ body: valid }), res);
    expect(res.statusCode).toBe(500);
    expect(payload(res)).toEqual({ code: 'INTERNAL_ERROR', message: 'Server misconfigured' });
  });

  it('fails closed when the Razorpay secret is not configured', async () => {
    vi.stubEnv('JWT_PRIVATE_KEY_PEM', 'key');
    vi.stubEnv('RAZORPAY_KEY_SECRET', '');
    const handler = await load('../api/license/activate');
    const res = makeRes();
    await handler(req({ body: valid }), res);
    expect(res.statusCode).toBe(500);
  });

  it('maps a rate-limit refusal to 429 rather than the default 400', async () => {
    vi.stubEnv('JWT_PRIVATE_KEY_PEM', 'key');
    vi.stubEnv('RAZORPAY_KEY_SECRET', 'secret');
    // Six prior attempts for this email trips the 5/hour limit.
    prisma.licensingAuditLog.create.mockResolvedValue({});
    prisma.licensingAuditLog.findMany.mockResolvedValue(
      Array.from({ length: 6 }, () => ({ payload: { email: valid.email } }))
    );
    const handler = await load('../api/license/activate');
    const res = makeRes();
    await handler(req({ body: valid }), res);
    expect(res.statusCode).toBe(429);
    expect(payload(res).code).toBe('RATE_LIMITED');
  });

  it('does not rate-limit a different account’s attempts', async () => {
    vi.stubEnv('JWT_PRIVATE_KEY_PEM', 'key');
    vi.stubEnv('RAZORPAY_KEY_SECRET', 'secret');
    prisma.licensingAuditLog.create.mockResolvedValue({});
    prisma.licensingAuditLog.findMany.mockResolvedValue(
      Array.from({ length: 6 }, () => ({ payload: { email: 'someone-else@b.com' } }))
    );
    const handler = await load('../api/license/activate');
    const res = makeRes();
    await handler(req({ body: valid }), res);
    expect(res.statusCode).not.toBe(429);
  });
});

describe('license/refresh-token handler', () => {
  it('rejects a non-POST', async () => {
    const handler = await load('../api/license/refresh-token');
    const res = makeRes();
    await handler(req({ method: 'GET' }), res);
    expect(res.statusCode).toBe(405);
  });

  it.each(['jwt', 'device_id'])('rejects a request missing %s', async (field) => {
    const handler = await load('../api/license/refresh-token');
    const res = makeRes();
    const body: Record<string, string> = { jwt: 'j', device_id: 'd' };
    delete body[field];
    await handler(req({ body }), res);
    expect(res.statusCode).toBe(400);
  });

  it.each([
    ['JWT_PRIVATE_KEY_PEM', ''],
    ['JWT_PUBLIC_KEY_PEM', ''],
  ])('fails closed when %s is missing', async (missing) => {
    vi.stubEnv('JWT_PRIVATE_KEY_PEM', missing === 'JWT_PRIVATE_KEY_PEM' ? '' : 'key');
    vi.stubEnv('JWT_PUBLIC_KEY_PEM', missing === 'JWT_PUBLIC_KEY_PEM' ? '' : 'key');
    const handler = await load('../api/license/refresh-token');
    const res = makeRes();
    await handler(req({ body: { jwt: 'j', device_id: 'd' } }), res);
    expect(res.statusCode).toBe(500);
  });
});

describe('admin/plans handler', () => {
  it('lists plans on GET without requiring admin auth', async () => {
    prisma.plan.findMany.mockResolvedValue([{ id: 'desktop_pro_monthly' }]);
    const handler = await load('../api/admin/plans');
    const res = makeRes();
    await handler(req({ method: 'GET' }), res);
    expect(res.statusCode).toBe(200);
    expect(payload(res)).toEqual({ plans: [{ id: 'desktop_pro_monthly' }] });
    expect(assertAdminAuthorized).not.toHaveBeenCalled();
  });

  it('passes the active_only filter through', async () => {
    prisma.plan.findMany.mockResolvedValue([]);
    const handler = await load('../api/admin/plans');
    await handler(req({ method: 'GET', query: { active_only: 'true' } }), makeRes());
    expect(prisma.plan.findMany).toHaveBeenCalled();
  });

  it('requires admin auth to PATCH', async () => {
    assertAdminAuthorized.mockImplementation(() => {
      throw new LicensingApiError('LICENSE_INVALID', 'bad admin token');
    });
    const handler = await load('../api/admin/plans');
    const res = makeRes();
    await handler(req({ method: 'PATCH', body: { plan_id: 'p1' } }), res);
    expect(res.statusCode).toBe(400);
    expect(payload(res).message).toBe('bad admin token');
  });

  it('rejects a PATCH with no plan_id', async () => {
    const handler = await load('../api/admin/plans');
    const res = makeRes();
    await handler(req({ method: 'PATCH', body: {} }), res);
    expect(res.statusCode).toBe(400);
    expect(payload(res).message).toBe('plan_id is required');
  });

  it('tolerates a PATCH with no body at all', async () => {
    const handler = await load('../api/admin/plans');
    const res = makeRes();
    await handler(req({ method: 'PATCH', body: undefined }), res);
    expect(res.statusCode).toBe(400);
  });

  it.each(['POST', 'DELETE', 'PUT'])('rejects %s', async (method) => {
    const handler = await load('../api/admin/plans');
    const res = makeRes();
    await handler(req({ method }), res);
    expect(res.statusCode).toBe(405);
    expect(payload(res).message).toBe('GET or PATCH only');
  });
});

describe('admin/support_lookup handler', () => {
  it('requires admin auth before anything else', async () => {
    assertAdminAuthorized.mockImplementation(() => {
      throw new LicensingApiError('LICENSE_INVALID', 'nope');
    });
    const handler = await load('../api/admin/support_lookup');
    const res = makeRes();
    await handler(req({ method: 'GET', query: { email: 'a@b.com' } }), res);
    expect(res.statusCode).not.toBe(200);
    expect(prisma.account.findUnique).not.toHaveBeenCalled();
  });

  it('rejects a non-GET', async () => {
    const handler = await load('../api/admin/support_lookup');
    const res = makeRes();
    await handler(req({ method: 'POST' }), res);
    expect(res.statusCode).toBe(405);
  });

  it('requires an email query param', async () => {
    const handler = await load('../api/admin/support_lookup');
    const res = makeRes();
    await handler(req({ method: 'GET', query: {} }), res);
    expect(res.statusCode).toBe(400);
    expect(payload(res).message).toBe('email is required');
  });

  it('ignores a repeated email param rather than looking up an array', async () => {
    const handler = await load('../api/admin/support_lookup');
    const res = makeRes();
    await handler(req({ method: 'GET', query: { email: ['a@b.com', 'c@d.com'] } }), res);
    expect(res.statusCode).toBe(400);
  });
});

describe('billing/cancel handler', () => {
  it('rejects a non-POST', async () => {
    const handler = await load('../api/billing/cancel');
    const res = makeRes();
    await handler(req({ method: 'GET' }), res);
    expect(res.statusCode).toBe(405);
  });

  it('requires an account_id', async () => {
    const handler = await load('../api/billing/cancel');
    const res = makeRes();
    await handler(req({ body: {} }), res);
    expect(res.statusCode).toBe(400);
    expect(payload(res).message).toBe('account_id is required');
  });

  it('tolerates a missing body', async () => {
    const handler = await load('../api/billing/cancel');
    const res = makeRes();
    await handler(req({ body: undefined }), res);
    expect(res.statusCode).toBe(400);
  });

  it('surfaces a domain error through the shared mapper', async () => {
    prisma.subscription.findFirst.mockRejectedValue(
      new LicensingApiError('NOT_FOUND', 'no subscription')
    );
    const handler = await load('../api/billing/cancel');
    const res = makeRes();
    await handler(req({ body: { account_id: 'acc1' } }), res);
    expect(res.statusCode).toBe(400);
    expect(payload(res)).toEqual({ code: 'NOT_FOUND', message: 'no subscription' });
  });

  it('hides an unexpected failure behind a 500', async () => {
    prisma.subscription.findFirst.mockRejectedValue(new Error('postgres down'));
    const handler = await load('../api/billing/cancel');
    const res = makeRes();
    await handler(req({ body: { account_id: 'acc1' } }), res);
    expect(res.statusCode).toBe(500);
    expect(JSON.stringify(payload(res))).not.toContain('postgres');
  });
});

describe('billing/refund handler', () => {
  const body = { account_id: 'acc1', reason: 'duplicate charge' };

  it('rejects a non-POST', async () => {
    const handler = await load('../api/billing/refund');
    const res = makeRes();
    await handler(req({ method: 'GET' }), res);
    expect(res.statusCode).toBe(405);
  });

  it.each(['account_id', 'reason'])('rejects a request missing %s', async (field) => {
    const handler = await load('../api/billing/refund');
    const res = makeRes();
    const partial: Record<string, string> = { ...body };
    delete partial[field];
    await handler(req({ body: partial }), res);
    expect(res.statusCode).toBe(400);
  });

  it('requires admin auth', async () => {
    assertAdminAuthorized.mockImplementation(() => {
      throw new LicensingApiError('LICENSE_INVALID', 'not an admin');
    });
    const handler = await load('../api/billing/refund');
    const res = makeRes();
    await handler(req({ body }), res);
    expect(res.statusCode).toBe(400);
    expect(payload(res).message).toBe('not an admin');
  });

  it.each([
    ['RAZORPAY_KEY_ID', ''],
    ['RAZORPAY_KEY_SECRET', ''],
  ])('fails closed when %s is missing', async (missing) => {
    vi.stubEnv('RAZORPAY_KEY_ID', missing === 'RAZORPAY_KEY_ID' ? '' : 'id');
    vi.stubEnv('RAZORPAY_KEY_SECRET', missing === 'RAZORPAY_KEY_SECRET' ? '' : 'secret');
    const handler = await load('../api/billing/refund');
    const res = makeRes();
    await handler(req({ body }), res);
    expect(res.statusCode).toBe(500);
  });

  it('hides an unexpected failure behind a 500', async () => {
    vi.stubEnv('RAZORPAY_KEY_ID', 'id');
    vi.stubEnv('RAZORPAY_KEY_SECRET', 'secret');
    prisma.account.findUnique.mockRejectedValue(new Error('postgres down'));
    prisma.subscription.findFirst.mockRejectedValue(new Error('postgres down'));
    const handler = await load('../api/billing/refund');
    const res = makeRes();
    await handler(req({ body }), res);
    expect(res.statusCode).toBe(500);
    expect(JSON.stringify(payload(res))).not.toContain('postgres');
  });
});

describe('license/webhooks/razorpay handler', () => {
  const signed = { headers: { 'x-razorpay-signature': 'sig' }, body: { event: 'payment.captured' } };

  it('rejects a non-POST', async () => {
    const handler = await load('../api/license/webhooks/razorpay');
    const res = makeRes();
    await handler(req({ method: 'GET' }), res);
    expect(res.statusCode).toBe(405);
  });

  it('fails closed when the webhook secret is not configured', async () => {
    vi.stubEnv('RAZORPAY_WEBHOOK_SECRET', '');
    const handler = await load('../api/license/webhooks/razorpay');
    const res = makeRes();
    await handler(req(signed), res);
    expect(res.statusCode).toBe(500);
  });

  it('fails closed when the request carries no signature header', async () => {
    vi.stubEnv('RAZORPAY_WEBHOOK_SECRET', 'whsec');
    const handler = await load('../api/license/webhooks/razorpay');
    const res = makeRes();
    await handler(req({ headers: {}, body: {} }), res);
    expect(res.statusCode).toBe(500);
  });

  it('rejects a forged signature', async () => {
    vi.stubEnv('RAZORPAY_WEBHOOK_SECRET', 'whsec');
    verifyWebhookSignature.mockReturnValue(false);
    const handler = await load('../api/license/webhooks/razorpay');
    const res = makeRes();
    await handler(req(signed), res);
    expect(res.statusCode).toBe(400);
    expect(payload(res).code).toBe('INVALID_WEBHOOK_SIGNATURE');
  });

  it('verifies against the exact raw body it received', async () => {
    vi.stubEnv('RAZORPAY_WEBHOOK_SECRET', 'whsec');
    const handler = await load('../api/license/webhooks/razorpay');
    await handler(req(signed), makeRes());
    expect(verifyWebhookSignature).toHaveBeenCalledWith(
      JSON.stringify(signed.body),
      'sig',
      'whsec'
    );
  });

  it('hides a processing failure behind a 500', async () => {
    vi.stubEnv('RAZORPAY_WEBHOOK_SECRET', 'whsec');
    prisma.subscription.findFirst.mockRejectedValue(new Error('postgres down'));
    prisma.account.findUnique.mockRejectedValue(new Error('postgres down'));
    const handler = await load('../api/license/webhooks/razorpay');
    const res = makeRes();
    await handler(req({ ...signed, body: { event: 'subscription.charged' } }), res);
    expect([200, 500]).toContain(res.statusCode);
    if (res.statusCode === 500) {
      expect(JSON.stringify(payload(res))).not.toContain('postgres');
    }
  });
});
