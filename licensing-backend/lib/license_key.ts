import { createHash } from 'node:crypto';

/// license_tokens.license_key_hash never stores the raw key (Doc 17 §4.2).
export function hashLicenseKey(licenseKey: string): string {
  return createHash('sha256').update(licenseKey).digest('hex');
}

/// Doc 30 TASK-LIC-002: "surfacing only the bound device's masked identifier,
/// never the other device's full UUID" on a DEVICE_ALREADY_BOUND rejection.
export function maskDeviceFingerprint(fingerprint: string): string {
  if (fingerprint.length <= 8) return '****';
  return `${fingerprint.slice(0, 4)}...${fingerprint.slice(-4)}`;
}
