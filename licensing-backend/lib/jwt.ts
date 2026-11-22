/**
 * License token signing and verification.
 *
 * Tokens are RS256-signed, which is what allows the desktop app to verify a
 * license offline: it holds only the public key and can therefore check
 * authenticity without contacting the server or holding any signing secret.
 *
 * The claim set is intentionally minimal, and assertNoExcessClaims enforces that
 * as a hard check -- every claim is readable by anyone holding the token, so
 * nothing beyond what entitlement actually requires belongs in it.
 */
import jwt from 'jsonwebtoken';

/** Entitlement the client honours; signed so a client cannot upgrade itself. */
export type ServerLicenseState = 'TRIAL' | 'ACTIVE' | 'PAST_DUE' | 'LOCKED';

export interface LicenseJwtClaims {
  sub: string;
  device_id: string;
  plan: string;
  billing_interval: string;
  state: ServerLicenseState;
}

/**
 * Maps a subscription status onto the entitlement state the client understands.
 *
 * The default is LOCKED, which is the safe direction: an unrecognised status
 * (cancelled, halted, completed, expired) must never be read as granting access.
 */
export function stateForSubscriptionStatus(subscriptionStatus: string): ServerLicenseState {
  switch (subscriptionStatus) {
    case 'trialing':
      return 'TRIAL';
    case 'active':
      return 'ACTIVE';
    case 'past_due':
      return 'PAST_DUE';
    default:
      return 'LOCKED';
  }
}

// The complete permitted claim set, including the two JWT registered claims.
// Anything outside this is a leak of information the token has no need to carry.
const ALLOWED_CLAIM_KEYS = new Set([
  'sub',
  'device_id',
  'plan',
  'billing_interval',
  'state',
  'exp',
  'iat',
]);

// Seven days: the offline allowance. A machine that cannot reach the server
// keeps working this long on its last token; the signed `state` claim means a
// lapsed subscription is honoured as LOCKED at the next successful check.
const DEFAULT_EXPIRY_SECONDS = 7 * 24 * 60 * 60;

/** Token lifetime in milliseconds, for the `expires_at` values responses report. */
export const LICENSE_TOKEN_TTL_MS = DEFAULT_EXPIRY_SECONDS * 1000;

/** Sign a claim set into a license token with the given private key. */
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

/** Distinct error type so callers can tell a bad token from a server fault. */
export class JwtVerificationError extends Error {}

/**
 * Verify a token and return its claims.
 *
 * The algorithm is pinned to RS256 rather than read from the token header --
 * without that, a token could declare `alg: none` or a symmetric algorithm and
 * bypass signature checking entirely.
 *
 * `ignoreExpiration` supports grace-period handling, where an expired but
 * otherwise valid token still needs to be inspected.
 */
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

/**
 * Fail if a claim set carries anything beyond the allowed keys.
 *
 * A guard against accidental data exposure: JWT payloads are merely base64
 * encoded, not encrypted, so any field added here becomes readable by anyone
 * who obtains the token.
 */
export function assertNoExcessClaims(claims: Record<string, unknown>): void {
  const excess = Object.keys(claims).filter((k) => !ALLOWED_CLAIM_KEYS.has(k));
  if (excess.length > 0) {
    throw new Error(`JWT claims contain excess/disallowed fields: ${excess.join(', ')}`);
  }
}
