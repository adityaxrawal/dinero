import { describe, it, expect } from 'vitest';
import { mapAppErrorToToast, getErrorMessage, getErrorToast } from '@/lib/errorMapping';

describe('mapAppErrorToToast', () => {
  it('maps LICENSE_LOCKED to the subscription-attention copy with a Settings action', () => {
    const result = mapAppErrorToToast({ code: 'LICENSE_LOCKED', message: 'grace expired' });
    expect(result.title).toBe('Your subscription needs attention');
    expect(result.actionTo).toBe('/settings');
    expect(result.actionLabel).toBeTruthy();
  });

  it('surfaces the backend message verbatim for VALIDATION_ERROR (field-level detail)', () => {
    const result = mapAppErrorToToast({
      code: 'VALIDATION_ERROR',
      message: 'amount must be positive',
    });
    expect(result.description).toBe('amount must be positive');
  });

  it('maps NETWORK_ERROR to generic connectivity copy, ignoring the raw message', () => {
    const result = mapAppErrorToToast({ code: 'NETWORK_ERROR', message: 'ECONNREFUSED' });
    expect(result.description).toMatch(/internet connection/i);
  });

  it('falls back to a generic "Something went wrong" for unmapped/unknown codes', () => {
    const result = mapAppErrorToToast({ code: 'UNKNOWN_ERROR', message: 'weird failure' });
    expect(result.title).toBe('Something went wrong');
    expect(result.description).toBe('weird failure');
  });

  it('falls back to generic description text when the message is empty', () => {
    const result = mapAppErrorToToast({ code: 'UNKNOWN_ERROR', message: '' });
    expect(result.description).toBe('An unexpected error occurred.');
  });

  it('does not attach an action for codes with no relevant navigation target', () => {
    const result = mapAppErrorToToast({ code: 'NETWORK_ERROR', message: '' });
    expect(result.actionTo).toBeUndefined();
  });
});

describe('getErrorMessage', () => {
  it('reads .message off an Error instance', () => {
    expect(getErrorMessage(new Error('boom'))).toBe('boom');
  });

  it('reads .message off a plain object', () => {
    expect(getErrorMessage({ message: 'ipc rejected' })).toBe('ipc rejected');
  });

  it.each([null, undefined, 'a string', 42, { message: 404 }])(
    'falls back to the default for %p',
    (thrown) => {
      expect(getErrorMessage(thrown)).toBe('An unexpected error occurred');
    }
  );

  it('honours a caller-supplied default', () => {
    expect(getErrorMessage(null, 'could not load statements')).toBe('could not load statements');
  });
});

describe('getErrorToast', () => {
  it('routes a code-carrying AppError through the code map', () => {
    const result = getErrorToast({ code: 'LICENSE_LOCKED', message: 'grace expired' });
    expect(result.title).toBe('Your subscription needs attention');
  });

  it('wraps a bare Error in a generic Error toast', () => {
    const result = getErrorToast(new Error('socket hang up'));
    expect(result.title).toBe('Error');
    expect(result.description).toBe('socket hang up');
  });

  it('uses the caller default when the thrown value carries nothing usable', () => {
    const result = getErrorToast('oops', 'statement upload failed');
    expect(result.description).toBe('statement upload failed');
  });
});
