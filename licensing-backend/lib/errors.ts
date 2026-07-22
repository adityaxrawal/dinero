// Doc 19 §4 Error Catalog -- the subset the Licensing Backend itself raises.
// Desktop-side codes (GMAIL_*, STATEMENT_*, etc.) never apply here.
export type LicensingErrorCode =
  | 'VALIDATION_ERROR'
  | 'NOT_FOUND'
  | 'LICENSE_INVALID'
  | 'DEVICE_ALREADY_BOUND'
  | 'DEVICE_MISMATCH'
  | 'PAYMENT_VERIFICATION_FAILED'
  | 'RATE_LIMITED'
  | 'INVALID_WEBHOOK_SIGNATURE'
  | 'INTERNAL_ERROR';

export class LicensingApiError extends Error {
  constructor(
    public readonly code: LicensingErrorCode,
    message: string,
    public readonly details?: Record<string, unknown>
  ) {
    super(message);
  }
}
