import { describe, it, expect } from 'vitest';
import { mapOauthError } from './mapOauthError';

describe('mapOauthError', () => {
  it('maps the oauth_timeout backend error to a friendly, actionable message', () => {
    expect(mapOauthError('Authentication error: oauth_timeout')).toBe('Connection timed out, try again.');
  });

  it('passes through other error messages unchanged', () => {
    expect(mapOauthError('Failed to store token')).toBe('Failed to store token');
  });

  it('falls back to a generic message when no message is given', () => {
    expect(mapOauthError(undefined)).toBe('Failed to connect Gmail. Please try again.');
    expect(mapOauthError(null)).toBe('Failed to connect Gmail. Please try again.');
    expect(mapOauthError('')).toBe('Failed to connect Gmail. Please try again.');
  });
});
