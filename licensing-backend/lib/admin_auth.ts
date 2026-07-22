// Every "internal admin-authenticated endpoint" (Doc 30 TASK-BILL-001/006/007)
// shares this one check. Placeholder infra: a single bearer token from env,
// no separate admin-user table -- appropriate for a solo-operator backend
// (Doc 17 §5 solo-developer operating context); revisit if/when a real
// admin-user model is ever needed.
import { LicensingApiError } from './errors';

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
