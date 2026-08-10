/**
 * Liveness probe for uptime monitoring.
 *
 * Checks database reachability rather than merely returning 200, so a deployment
 * that is running but cannot reach its database reports unhealthy instead of
 * appearing fine while failing every real request.
 */
import { withRequestLogging } from '../lib/request_logging';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../lib/db';

export interface HealthResult {
  status: 'ok' | 'degraded';
  db_latency_ms: number;
}

export type HealthDb = {
  $queryRaw<T = unknown>(query: TemplateStringsArray): Promise<T>;
};

/**
 * Checks database reachability.
 *
 * Verifies the dependency rather than merely returning 200, so a deployment that
 * is running but cannot reach its database reports unhealthy instead of looking
 * fine while failing every real request.
 */
export async function checkHealth(db: HealthDb): Promise<HealthResult> {
  const start = Date.now();
  try {
    await db.$queryRaw`SELECT 1`;
    return { status: 'ok', db_latency_ms: Date.now() - start };
  } catch {
    return { status: 'degraded', db_latency_ms: Date.now() - start };
  }
}

/**
 * HTTP entry point: validates the request, delegates, and maps errors to statuses.
 */
async function handler(req: VercelRequest, res: VercelResponse) {
  const result = await checkHealth(prisma);
  res.status(result.status === 'ok' ? 200 : 503).json(result);
}

export default withRequestLogging('health', handler);
