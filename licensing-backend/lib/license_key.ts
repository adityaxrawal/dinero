// Corrected during TASK-BILL-002 (real conflict found and resolved, see
// Doc 30 changelog): this system has no user-facing "license_key" concept
// at all -- activation is a direct Razorpay payment confirmation, and
// device_id is the sole lookup key everywhere. `hashLicenseKey` was removed
// along with the license-key model it belonged to; only the masking helper
// (still needed for DEVICE_ALREADY_BOUND rejections) remains.

/// Doc 30 TASK-LIC-002: "surfacing only the bound device's masked identifier,
/// never the other device's full UUID" on a DEVICE_ALREADY_BOUND rejection.
export function maskDeviceFingerprint(fingerprint: string): string {
  if (fingerprint.length <= 8) return '****';
  return `${fingerprint.slice(0, 4)}...${fingerprint.slice(-4)}`;
}
