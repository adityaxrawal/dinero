// The HTTP wiring layer. The existing suites import each endpoint's pure
// function (`validateLicense`, `startTrial`, `createOrder`, `reissueToken`)
// and assert its behaviour directly, which leaves the `handler` around it --
// method guard, required-field guard, missing-config guard, admin auth, and
// the error->status mapping -- entirely unexercised.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import type { VercelRequest, VercelResponse } from '@vercel/node';

vi.mock('../lib/db', () => ({ prisma: {} }));

const getRazorpayCredentials = vi.fn();
vi.mock('../lib/razorpay', () => ({
  getRazorpayCredentials: () => getRazorpayCredentials(),
  realRazorpayOrders: vi.fn(() => ({})),
  realRazorpaySubscriptions: vi.fn(() => ({})),
}));

import validateHandler from '../api/license/validate';
import startTrialHandler from '../api/license/start-trial';
import createOrderHandler from '../api/billing/create-order';
import reissueHandler from '../api/admin/support_reissue_token';

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

const req = (over: Partial<VercelRequest> = {}) =>
  ({ method: 'POST', headers: {}, body: {}, ...over }) as unknown as VercelRequest;

const env = { ...process.env };

beforeEach(() => {
  vi.clearAllMocks();
  process.env.JWT_PRIVATE_KEY_PEM = '';
  getRazorpayCredentials.mockReturnValue(null);
});

afterEach(() => {
  process.env = { ...env };
});

const POST_ONLY = [
  ['license/validate', validateHandler],
  ['license/start-trial', startTrialHandler],
  ['billing/create-order', createOrderHandler],
] as const;

describe('handler entrypoints — method guard', () => {
  it.each(POST_ONLY)('%s rejects GET with 405', async (_name, handler) => {
    const res = makeRes();
    await handler(req({ method: 'GET' }), res);

    expect(res.statusCode).toBe(405);
    expect(res.body).toMatchObject({ code: 'VALIDATION_ERROR' });
  });
});

describe('handler entrypoints — required fields', () => {
  it.each([
    ['license/validate', validateHandler, 'device_id'],
    ['license/start-trial', startTrialHandler, 'email'],
    ['billing/create-order', createOrderHandler, 'email'],
  ] as const)('%s rejects a body missing %s', async (_name, handler, field) => {
    const res = makeRes();
    await handler(req({ body: {} }), res);

    expect(res.statusCode).toBe(400);
    expect(res.body).toMatchObject({ message: `${field} is required` });
  });

  it('license/validate rejects an entirely absent body', async () => {
    const res = makeRes();
    await validateHandler(req({ body: undefined }), res);

    expect(res.statusCode).toBe(400);
  });
});

describe('handler entrypoints — server misconfiguration', () => {
  it('license/validate refuses to sign without a private key', async () => {
    delete process.env.JWT_PRIVATE_KEY_PEM;
    const res = makeRes();
    await validateHandler(req({ body: { device_id: 'device-A' } }), res);

    expect(res.statusCode).toBe(500);
    expect(res.body).toMatchObject({ code: 'INTERNAL_ERROR', message: 'Server misconfigured' });
  });

  it('license/start-trial refuses to sign without a private key', async () => {
    delete process.env.JWT_PRIVATE_KEY_PEM;
    const res = makeRes();
    await startTrialHandler(req({ body: { email: 'a@b.com', device_id: 'device-A' } }), res);

    expect(res.statusCode).toBe(500);
  });

  it('billing/create-order refuses to run without Razorpay credentials', async () => {
    getRazorpayCredentials.mockReturnValue(null);
    const res = makeRes();
    await createOrderHandler(req({ body: { email: 'a@b.com', plan_id: 'pro' } }), res);

    expect(res.statusCode).toBe(500);
    expect(res.body).toMatchObject({ code: 'INTERNAL_ERROR' });
  });

  it('checks the field guard before the credentials guard', async () => {
    // Order matters: a caller sending a malformed body should get a 400
    // describing their mistake, not a 500 blaming the server.
    getRazorpayCredentials.mockReturnValue(null);
    const res = makeRes();
    await createOrderHandler(req({ body: {} }), res);

    expect(res.statusCode).toBe(400);
  });
});

describe('admin/support_reissue_token — auth', () => {
  it('never reaches the reissue logic without a valid admin token', async () => {
    process.env.ADMIN_API_TOKEN = 'admin-secret';
    const res = makeRes();
    await reissueHandler(req({ body: { email: 'a@b.com', new_device_id: 'device-B' } }), res);

    expect(res.statusCode).not.toBe(200);
    expect(res.body).toMatchObject({ message: 'Admin authorization required' });
  });

  it('asserts auth before validating the body', async () => {
    // An unauthorized caller must not be able to use field-validation
    // responses to probe the endpoint's expected shape: an empty body still
    // reports the auth failure, not "email is required".
    process.env.ADMIN_API_TOKEN = 'admin-secret';
    const res = makeRes();
    await reissueHandler(req({ headers: { authorization: 'Bearer wrong' }, body: {} }), res);

    expect(res.body).toMatchObject({ message: 'Admin authorization required' });
  });

  it('accepts the configured admin token past the auth gate', async () => {
    process.env.ADMIN_API_TOKEN = 'admin-secret';
    const res = makeRes();
    await reissueHandler(
      req({ headers: { authorization: 'Bearer admin-secret' }, body: {} }),
      res
    );

    // Past auth, so it now complains about the body instead.
    expect(res.body).toMatchObject({ message: 'email is required' });
  });

  // Characterisation, not endorsement. `assertAdminAuthorized` throws
  // VALIDATION_ERROR for an auth failure and INTERNAL_ERROR when
  // ADMIN_API_TOKEN is unset, and `handleAdminSupportError` maps everything
  // except NOT_FOUND to 400. So this endpoint answers 400 both to an
  // unauthorized caller (arguably 401) and to its own misconfiguration
  // (arguably 500) -- unlike the license/billing endpoints above, which do
  // return 500 for "Server misconfigured". Locked in so a deliberate change
  // to these codes is visible rather than silent.
  it('currently answers 400 for an unauthorized caller', async () => {
    process.env.ADMIN_API_TOKEN = 'admin-secret';
    const res = makeRes();
    await reissueHandler(req({ headers: { authorization: 'Bearer wrong' }, body: {} }), res);

    expect(res.statusCode).toBe(400);
  });

  it('currently answers 400 when ADMIN_API_TOKEN is not configured', async () => {
    delete process.env.ADMIN_API_TOKEN;
    const res = makeRes();
    await reissueHandler(req({ headers: { authorization: 'Bearer anything' }, body: {} }), res);

    expect(res.statusCode).toBe(400);
    expect(res.body).toMatchObject({ message: 'ADMIN_API_TOKEN not configured' });
  });
});
