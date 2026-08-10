/**
 * Admin metrics endpoint: billing and trial-funnel figures in one response.
 *
 * Behind the admin bearer token. Aggregates only -- no per-user rows are
 * returned, so the operational dashboard never becomes a way to browse
 * individual customer data.
 */
import { withRequestLogging } from '../../lib/request_logging';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import { assertAdminAuthorized } from '../../lib/admin_auth';
import { sendApiError } from '../../lib/errors';
import {
  paidMau,
  trialToPaidConversionRate,
  monthlyChurnRate,
  mrr,
} from '../../lib/billing_metrics';

/**
 * HTTP entry point: validates the request, delegates, and maps errors to statuses.
 */
async function handler(req: VercelRequest, res: VercelResponse) {
  try {
    assertAdminAuthorized(req.headers.authorization);
    const [mau, conversionRate90d, churnRate, monthlyRecurringRevenue] = await Promise.all([
      paidMau(prisma),
      trialToPaidConversionRate(prisma, 90),
      monthlyChurnRate(prisma),
      mrr(prisma),
    ]);
    res.status(200).json({
      paid_mau: mau,
      conversion_rate_90d: conversionRate90d,
      monthly_churn_rate: churnRate,
      mrr: monthlyRecurringRevenue,
    });
  } catch (e) {
    sendApiError(res, e);
  }
}

export default withRequestLogging('admin/metrics', handler);
