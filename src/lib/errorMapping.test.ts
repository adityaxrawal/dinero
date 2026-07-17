import { describe, it, expect } from 'vitest';
import { mapAppErrorToToast } from './errorMapping';

describe('mapAppErrorToToast', () => {
  it('maps LICENSE_LOCKED to the subscription-attention copy with a Settings action', () => {
    const result = mapAppErrorToToast({ code: 'LICENSE_LOCKED', message: 'grace expired' });
    expect(result.title).toBe('Your subscription needs attention');
    expect(result.actionTo).toBe('/settings');
    expect(result.actionLabel).toBeTruthy();
  });

  it('surfaces the backend message verbatim for VALIDATION_ERROR (field-level detail)', () => {
    const result = mapAppErrorToToast({ code: 'VALIDATION_ERROR', message: 'amount must be positive' });
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
