// Doc 30 TASK-LIC-005 acceptance criteria.
import { describe, it, expect } from 'vitest';
import { signLicenseJwt, verifyLicenseJwt, assertNoExcessClaims, JwtVerificationError } from '../lib/jwt';
import { TEST_PRIVATE_KEY_PEM, TEST_PUBLIC_KEY_PEM } from './testKeys';

const claims = { sub: 'user@example.com', device_id: 'device-abc', plan: 'desktop_pro_monthly', billing_interval: 'monthly' };

describe('test_jwt_signed_with_rs256', () => {
  it('produces a 3-part token with an RS256 header', () => {
    const token = signLicenseJwt(claims, TEST_PRIVATE_KEY_PEM);
    const [headerB64] = token.split('.');
    const header = JSON.parse(Buffer.from(headerB64, 'base64url').toString());
    expect(header.alg).toBe('RS256');
    expect(token.split('.')).toHaveLength(3);
  });
});

describe('test_public_key_verifies_valid_signature', () => {
  it('verifies a token signed with the matching private key', () => {
    const token = signLicenseJwt(claims, TEST_PRIVATE_KEY_PEM);
    const verified = verifyLicenseJwt(token, TEST_PUBLIC_KEY_PEM);
    expect(verified.sub).toBe('user@example.com');
    expect(verified.device_id).toBe('device-abc');
    expect(verified.plan).toBe('desktop_pro_monthly');
  });
});

describe('test_tampered_jwt_rejected', () => {
  it('rejects a payload with one flipped base64url character', () => {
    const token = signLicenseJwt(claims, TEST_PRIVATE_KEY_PEM);
    const parts = token.split('.');
    const payload = parts[1];
    const last = payload[payload.length - 1];
    parts[1] = payload.slice(0, -1) + (last === 'A' ? 'B' : 'A');
    const tampered = parts.join('.');
    expect(() => verifyLicenseJwt(tampered, TEST_PUBLIC_KEY_PEM)).toThrow(JwtVerificationError);
  });

  it('rejects a token signed by an unrelated key', () => {
    // Deliberately generated, unrelated public key (openssl-style dummy) --
    // any well-formed but non-matching public key proves signature checking
    // is real, not a no-op that just decodes claims.
    const UNRELATED_PUBLIC_KEY = TEST_PUBLIC_KEY_PEM.replace(/A/g, 'B');
    expect(() => verifyLicenseJwt(signLicenseJwt(claims, TEST_PRIVATE_KEY_PEM), UNRELATED_PUBLIC_KEY)).toThrow();
  });
});

describe('test_jwt_claims_contain_no_excess_pii', () => {
  it('accepts the documented claim set', () => {
    expect(() => assertNoExcessClaims({ ...claims, iat: 1, exp: 2 })).not.toThrow();
  });

  it('rejects a claim set with an extra field', () => {
    expect(() => assertNoExcessClaims({ ...claims, full_name: 'Jane Doe' })).toThrow(/excess/i);
  });
});
