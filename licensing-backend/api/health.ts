// Doc 30 TASK-OPS-003: GET /api/health
//
// A synthetic-check-friendly endpoint for uptime monitors — deliberately the
// one Licensing Backend endpoint requiring no auth, since its whole point is
// to be pollable by an external monitor with zero account access. Response
// body is limited to status/latency metadata only: never an account email,
// license key, device fingerprint, or any other identity/billing field.
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../lib/db';

export interface HealthResult {
  status: 'ok' | 'degraded';
  db_latency_ms: number;
}

export type HealthDb = {
  $queryRaw<T = unknown>(query: TemplateStringsArray): Promise<T>;
};

export async function checkHealth(db: HealthDb): Promise<HealthResult> {
  const start = Date.now();
  try {
    await db.$queryRaw`SELECT 1`;
    return { status: 'ok', db_latency_ms: Date.now() - start };
  } catch {
    return { status: 'degraded', db_latency_ms: Date.now() - start };
  }
}

export default async function handler(req: VercelRequest, res: VercelResponse) {
  const result = await checkHealth(prisma);
  res.status(result.status === 'ok' ? 200 : 503).json(result);
}
