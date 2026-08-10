/**
 * Redaction helpers for values that must not appear whole in logs.
 *
 * Device fingerprints and email addresses both identify a user, so anything
 * written to an operational log passes through here first. Each keeps just
 * enough of the original to correlate entries during support work, without
 * being reversible to the underlying identity.
 */
export function maskDeviceFingerprint(fingerprint: string): string {
  if (fingerprint.length <= 8) return '****';
  return `${fingerprint.slice(0, 4)}...${fingerprint.slice(-4)}`;
}

/**
 * Masks an email, keeping the first and last local characters and the domain.
 *
 * Enough to correlate log entries during support work, not enough to recover the
 * address. Very short local parts are masked entirely, since one character either
 * side would reveal the whole thing.
 */
export function maskEmail(email: string): string {
  const at = email.indexOf('@');
  if (at <= 0) return '***';
  const local = email.slice(0, at);
  const domain = email.slice(at);
  if (local.length <= 2) return `${'*'.repeat(local.length)}${domain}`;
  return `${local[0]}${'*'.repeat(local.length - 2)}${local[local.length - 1]}${domain}`;
}
