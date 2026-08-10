/**
 * Error taxonomy and the shared API error response shape.
 *
 * LicensingApiError carries a machine-readable code so the desktop client can
 * branch on the failure rather than parse prose. The send helper enforces one
 * rule throughout: known errors surface their code and message, while anything
 * unrecognised collapses to a generic 500. That asymmetry is deliberate -- an
 * unexpected exception may carry stack traces or database detail, and this is
 * the boundary where that stops.
 */
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

interface VercelResponseLike {
  status(code: number): { json(body: unknown): void };
}

/**
 * Writes an error response, revealing detail only for known error types.
 *
 * The asymmetry is deliberate: an unrecognised exception may carry a stack trace
 * or database detail, and this is the boundary where that stops.
 */
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
