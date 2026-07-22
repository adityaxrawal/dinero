import { randomBytes } from 'node:crypto';

/// Human-readable license key, shown to the user exactly once (at trial
/// start / purchase) -- only its SHA-256 hash is ever persisted (Doc 17 §4.2).
export function generateLicenseKey(): string {
  const groups = Array.from({ length: 4 }, () => randomBytes(2).toString('hex').toUpperCase());
  return `DINERO-${groups.join('-')}`;
}
