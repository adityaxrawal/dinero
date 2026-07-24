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

// Every handler's catch block ends the same way: a LicensingApiError maps to
// its declared status (400 by default, a few endpoints special-case one code
// to a sharper status), anything else is a 500 with no details leaked.
interface VercelResponseLike {
  status(code: number): { json(body: unknown): void };
}

export function sendApiError(
  res: VercelResponseLike,
  e: unknown,
  options?: { statusFor?: (code: LicensingErrorCode) => number; includeDetails?: boolean }
): void {
  if (e instanceof LicensingApiError) {
    const status = options?.statusFor?.(e.code) ?? 400;
    res
      .status(status)
      .json(
        options?.includeDetails
          ? { code: e.code, message: e.message, details: e.details }
          : { code: e.code, message: e.message }
      );
    return;
  }
  res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Unexpected error' });
}
