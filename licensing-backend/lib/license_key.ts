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

/// Doc 30 TASK-OPS-006 / Doc 43 §3 action 7: a support lookup result must
/// never surface a full email in its response body, even to the one
/// authenticated admin -- only enough to confirm the operator found the
/// right account. `u***r@example.com` keeps the first/last character of the
/// local part; anything shorter than that just masks the whole local part.
export function maskEmail(email: string): string {
  const at = email.indexOf('@');
  if (at <= 0) return '***';
  const local = email.slice(0, at);
  const domain = email.slice(at);
  if (local.length <= 2) return `${'*'.repeat(local.length)}${domain}`;
  return `${local[0]}${'*'.repeat(local.length - 2)}${local[local.length - 1]}${domain}`;
}
