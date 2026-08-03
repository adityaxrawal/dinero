import { describe, it, expect, vi } from 'vitest';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { requirePostWithFields } from '../lib/api_helpers';
import { LicensingApiError, sendApiError } from '../lib/errors';

function makeRes() {
  const json = vi.fn();
  const res = { status: vi.fn().mockReturnValue({ json }) };
  return { res: res as unknown as VercelResponse, status: res.status, json };
}

const post = (body: unknown) => ({ method: 'POST', body }) as VercelRequest;

describe('requirePostWithFields', () => {
  it('accepts a POST carrying every required field', () => {
    const { res, status } = makeRes();
    expect(requirePostWithFields(post({ device_id: 'd1', key: 'k' }), res, ['device_id', 'key'])).toBe(true);
    expect(status).not.toHaveBeenCalled();
  });

  it('accepts a POST when nothing is required', () => {
    const { res } = makeRes();
    expect(requirePostWithFields(post({}), res, [])).toBe(true);
  });

  it.each(['GET', 'PUT', 'DELETE', 'PATCH'])('rejects %s with 405', (method) => {
    const { res, status, json } = makeRes();
    expect(requirePostWithFields({ method, body: {} } as VercelRequest, res, [])).toBe(false);
    expect(status).toHaveBeenCalledWith(405);
    expect(json).toHaveBeenCalledWith({ code: 'VALIDATION_ERROR', message: 'POST only' });
  });

  it('rejects a missing field with 400 and names it', () => {
    const { res, status, json } = makeRes();
    expect(requirePostWithFields(post({ device_id: 'd1' }), res, ['device_id', 'license_key'])).toBe(false);
    expect(status).toHaveBeenCalledWith(400);
    expect(json).toHaveBeenCalledWith({
      code: 'VALIDATION_ERROR',
      message: 'license_key is required',
    });
  });

  it('rejects an absent body', () => {
    const { res, status } = makeRes();
    expect(requirePostWithFields(post(undefined), res, ['device_id'])).toBe(false);
    expect(status).toHaveBeenCalledWith(400);
  });

  it.each([['', 'empty string'], [null, 'null'], [0, 'zero']])(
    'treats a %s field value as missing (%s)',
    (value) => {
      const { res, status } = makeRes();
      expect(requirePostWithFields(post({ device_id: value }), res, ['device_id'])).toBe(false);
      expect(status).toHaveBeenCalledWith(400);
    }
  );

  it('reports the first missing field only', () => {
    const { res, json } = makeRes();
    requirePostWithFields(post({}), res, ['first', 'second']);
    expect(json).toHaveBeenCalledTimes(1);
    expect(json).toHaveBeenCalledWith(expect.objectContaining({ message: 'first is required' }));
  });
});

describe('sendApiError', () => {
  it('maps a LicensingApiError to 400 by default', () => {
    const { res, status, json } = makeRes();
    sendApiError(res, new LicensingApiError('LICENSE_INVALID', 'No license bound'));
    expect(status).toHaveBeenCalledWith(400);
    expect(json).toHaveBeenCalledWith({ code: 'LICENSE_INVALID', message: 'No license bound' });
  });

  it('honours a per-endpoint status override', () => {
    const { res, status } = makeRes();
    sendApiError(res, new LicensingApiError('NOT_FOUND', 'gone'), {
      statusFor: (code) => (code === 'NOT_FOUND' ? 404 : 400),
    });
    expect(status).toHaveBeenCalledWith(404);
  });

  it('falls back to 400 when the override does not cover the code', () => {
    const { res, status } = makeRes();
    sendApiError(res, new LicensingApiError('RATE_LIMITED', 'slow down'), {
      statusFor: () => undefined as unknown as number,
    });
    expect(status).toHaveBeenCalledWith(400);
  });

  it('omits details unless explicitly requested', () => {
    const { res, json } = makeRes();
    sendApiError(res, new LicensingApiError('VALIDATION_ERROR', 'bad', { field: 'device_id' }));
    expect(json).toHaveBeenCalledWith({ code: 'VALIDATION_ERROR', message: 'bad' });
  });

  it('includes details when asked', () => {
    const { res, json } = makeRes();
    sendApiError(res, new LicensingApiError('VALIDATION_ERROR', 'bad', { field: 'device_id' }), {
      includeDetails: true,
    });
    expect(json).toHaveBeenCalledWith({
      code: 'VALIDATION_ERROR',
      message: 'bad',
      details: { field: 'device_id' },
    });
  });

  it.each([
    ['a bare Error', new Error('db connection lost')],
    ['a thrown string', 'kaboom'],
    ['null', null],
  ])('leaks nothing for %s — 500 with generic copy', (_label, thrown) => {
    const { res, status, json } = makeRes();
    sendApiError(res, thrown);
    expect(status).toHaveBeenCalledWith(500);
    expect(json).toHaveBeenCalledWith({ code: 'INTERNAL_ERROR', message: 'Unexpected error' });
  });

  it('does not leak the original message on an unexpected error', () => {
    const { res, json } = makeRes();
    sendApiError(res, new Error('postgres://user:secret@host'));
    expect(JSON.stringify(json.mock.calls)).not.toContain('secret');
  });
});
