// Doc 30 TASK-LIC-005 (claim shape corrected during a real conflict found
// while implementing this task): RS256 signing/verification. Claim shape
// matches Doc 22 §10.3 / Doc 19 §14.2 exactly -- the shape already built
// into the desktop app's verifier, src-tauri/src/licensing/jwt.rs's
// LicenseClaims { sub, device_id, plan, billing_interval, exp }. Doc 30's
// original text for this task described a different, PII-free shape
// (license_key/hardware_uuid/state instead of sub/device_id/billing_interval)
// that directly contradicted the already-shipped, tested desktop verifier --
// resolved in favor of Doc 22 (the declared domain owner for JWT/device-
// binding design) per Aditya's decision; Doc 30 itself corrected to match.
import jwt from 'jsonwebtoken';

export interface LicenseJwtClaims {
  /** The account's email — Doc 22 §10.3 accepts this as the JWT's identity
   * claim; the token lives in the desktop app's encrypted SQLite (Doc 18
   * §4.22), not in transit or in Keychain, so this is not a bare-text
   * exposure risk the way an unencrypted store would be. */
  sub: string;
  device_id: string;
  plan: string;
  billing_interval: string;
}

const ALLOWED_CLAIM_KEYS = new Set(['sub', 'device_id', 'plan', 'billing_interval', 'exp', 'iat']);

/// Doc 30 TASK-LIC-002/003: 24-72h expiry, requiring periodic revalidation.
const DEFAULT_EXPIRY_SECONDS = 48 * 60 * 60; // 48h, midpoint of the 24-72h band

export function signLicenseJwt(
  claims: LicenseJwtClaims,
  privateKeyPem: string,
  expirySeconds: number = DEFAULT_EXPIRY_SECONDS
): string {
  return jwt.sign(claims, privateKeyPem, { algorithm: 'RS256', expiresIn: expirySeconds });
}

export interface VerifiedLicenseJwt extends LicenseJwtClaims {
  iat: number;
  exp: number;
}

export class JwtVerificationError extends Error {}

/// Verifies signature + standard claims (exp/iat). Does NOT check
/// device_id match or subscription status -- callers (activate/validate/
/// refresh) do that against their own request context.
export function verifyLicenseJwt(
  token: string,
  publicKeyPem: string,
  opts: { ignoreExpiration?: boolean } = {}
): VerifiedLicenseJwt {
  let decoded: unknown;
  try {
    decoded = jwt.verify(token, publicKeyPem, {
      algorithms: ['RS256'],
      ignoreExpiration: opts.ignoreExpiration ?? false,
    });
  } catch (e) {
    throw new JwtVerificationError(e instanceof Error ? e.message : 'invalid token');
  }
  if (typeof decoded !== 'object' || decoded === null) {
    throw new JwtVerificationError('malformed payload');
  }
  return decoded as VerifiedLicenseJwt;
}

/// Doc 30 TASK-LIC-005 acceptance (corrected): claims never include anything
/// beyond sub/device_id/plan/billing_interval/exp/iat -- static guard so a
/// future edit adding a stray field (full_name, address, ...) fails loudly.
export function assertNoExcessClaims(claims: Record<string, unknown>): void {
  const excess = Object.keys(claims).filter((k) => !ALLOWED_CLAIM_KEYS.has(k));
  if (excess.length > 0) {
    throw new Error(`JWT claims contain excess/disallowed fields: ${excess.join(', ')}`);
  }
}
