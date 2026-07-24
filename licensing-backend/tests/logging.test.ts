// Doc 30 TASK-OPS-007 acceptance criteria.
import { describe, it, expect, vi, afterEach } from 'vitest';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { withRequestLogging } from '../lib/request_logging';
import { consoleEmailSender } from '../lib/email';
import { maskEmail } from '../lib/license_key';

function fakeRes(): VercelResponse {
  const res: Partial<VercelResponse> = {
    statusCode: 200,
    setHeader: vi.fn(),
    json: vi.fn(),
  };
  res.status = vi.fn().mockImplementation((code: number) => {
    res.statusCode = code;
    return res as VercelResponse;
  });
  return res as VercelResponse;
}

describe('test_logs_are_structured_and_redacted', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('logs exactly one structured JSON line with request_id/endpoint/status/latency_ms and nothing else', async () => {
    const logSpy = vi.spyOn(console, 'log').mockImplementation(() => {});
    const handler = vi.fn(async (_req: VercelRequest, res: VercelResponse) => {
      res.status(200).json({ ok: true });
    });
    const wrapped = withRequestLogging('license/validate', handler);

    await wrapped({} as VercelRequest, fakeRes());

    expect(logSpy).toHaveBeenCalledTimes(1);
    const logged = JSON.parse(logSpy.mock.calls[0][0] as string);
    expect(Object.keys(logged).sort()).toEqual(['endpoint', 'latency_ms', 'request_id', 'status']);
    expect(logged.endpoint).toBe('license/validate');
    expect(logged.status).toBe(200);
    expect(typeof logged.request_id).toBe('string');
    expect(typeof logged.latency_ms).toBe('number');
  });

  it('sets a distinct X-Request-Id response header so a support ticket can be correlated to a log line', async () => {
    const handler = vi.fn(async (_req: VercelRequest, res: VercelResponse) => {
      res.status(200).json({});
    });
    const res = fakeRes();
    await withRequestLogging('health', handler)({} as VercelRequest, res);
    expect(res.setHeader).toHaveBeenCalledWith('X-Request-Id', expect.any(String));
  });

  it('logs the line even when the handler throws, using whatever status was set before the throw', async () => {
    const logSpy = vi.spyOn(console, 'log').mockImplementation(() => {});
    const handler = vi.fn(async (_req: VercelRequest, res: VercelResponse) => {
      res.status(500);
      throw new Error('boom');
    });
    await expect(
      withRequestLogging('admin/support_lookup', handler)({} as VercelRequest, fakeRes())
    ).rejects.toThrow('boom');
    expect(logSpy).toHaveBeenCalledTimes(1);
    const logged = JSON.parse(logSpy.mock.calls[0][0] as string);
    expect(logged.status).toBe(500);
  });
});

describe('test_sensitive_fields_are_never_logged', () => {
  it('the placeholder email sender masks the recipient address rather than logging it raw', async () => {
    const logSpy = vi.spyOn(console, 'log').mockImplementation(() => {});
    await consoleEmailSender.send({ to: 'realuser@example.com', subject: 'hi', body: 'body' });
    const [line] = logSpy.mock.calls[0] as [string];
    expect(line).not.toContain('realuser@example.com');
    expect(line).toContain(maskEmail('realuser@example.com'));
    logSpy.mockRestore();
  });

  it('withRequestLogging never logs the request body, response body, or headers -- only 4 fixed fields', async () => {
    const logSpy = vi.spyOn(console, 'log').mockImplementation(() => {});
    const handler = vi.fn(async (_req: VercelRequest, res: VercelResponse) => {
      res.status(200).json({ email: 'realuser@example.com', jwt: 'super.secret.jwt' });
    });
    const req = {
      body: { email: 'realuser@example.com', device_id: 'hw-uuid-1' },
    } as unknown as VercelRequest;
    await withRequestLogging('license/activate', handler)(req, fakeRes());

    const logged = logSpy.mock.calls[0][0] as string;
    expect(logged).not.toContain('realuser@example.com');
    expect(logged).not.toContain('super.secret.jwt');
    expect(logged).not.toContain('hw-uuid-1');
    logSpy.mockRestore();
  });
});
