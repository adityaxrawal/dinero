import { withRequestLogging } from '../../lib/request_logging';
// Doc 30 TASK-BILL-008: Vercel Cron entrypoint (see vercel.json's `crons`).
// Vercel signs cron requests with a bearer token matching CRON_SECRET --
// verified here so this endpoint can't be triggered by an arbitrary caller.
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import { runBillingReconciliation } from '../../jobs/billing_reconciliation';
import { realRazorpaySubscriptions, getRazorpayCredentials } from '../../lib/razorpay';

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
