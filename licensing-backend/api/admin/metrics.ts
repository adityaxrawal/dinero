import { withRequestLogging } from '../../lib/request_logging';
// Doc 30 TASK-BILL-007: internal admin-authenticated reporting endpoint.
// Aggregate figures only -- never per-account breakdowns.
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
