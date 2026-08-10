/**
 * Scheduled entry point for billing reconciliation.
 *
 * A thin HTTP wrapper invoked by the platform's cron scheduler; the work itself
 * lives in the jobs module so it can be tested and invoked independently of the
 * scheduling mechanism.
 */
import { withRequestLogging } from '../../lib/request_logging';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import { runBillingReconciliation } from '../../jobs/billing_reconciliation';
import { realRazorpaySubscriptions, getRazorpayCredentials } from '../../lib/razorpay';

/**
 * Scheduled entry point invoked by the platform's cron.
 */
async function handler(req: VercelRequest, res: VercelResponse) {
  const cronSecret = process.env.CRON_SECRET;
  if (cronSecret && req.headers.authorization !== `Bearer ${cronSecret}`) {
    res.status(401).json({ code: 'VALIDATION_ERROR', message: 'Unauthorized' });
    return;
  }
  const credentials = getRazorpayCredentials();
  if (!credentials) {
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Server misconfigured' });
    return;
  }
  const { keyId, keySecret } = credentials;
  const summary = await runBillingReconciliation(
    prisma,
    realRazorpaySubscriptions(keyId, keySecret)
  );
  res.status(200).json(summary);
}

export default withRequestLogging('cron/billing-reconciliation', handler);
