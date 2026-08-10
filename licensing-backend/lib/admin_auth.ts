/**
 * Bearer-token guard for the admin endpoints.
 *
 * A missing ADMIN_API_TOKEN is treated as an internal error rather than a
 * rejection, which is the important distinction: an unconfigured server must
 * fail closed and loudly, not silently deny every request as if the caller were
 * at fault.
 */
import { LicensingApiError } from './errors';

/**
 * Rejects a request lacking the admin bearer token.
 *
 * An unconfigured token is an internal error rather than a rejection: the server
 * must fail closed and loudly, not silently deny every caller as if they were at
 * fault.
 */
export function assertAdminAuthorized(authorizationHeader: string | undefined): void {
  const expected = process.env.ADMIN_API_TOKEN;
  if (!expected) {
    throw new LicensingApiError('INTERNAL_ERROR', 'ADMIN_API_TOKEN not configured');
  }
  const provided = authorizationHeader?.replace(/^Bearer\s+/i, '');
  if (provided !== expected) {
    throw new LicensingApiError('VALIDATION_ERROR', 'Admin authorization required');
  }
}
