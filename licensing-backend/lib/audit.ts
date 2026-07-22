// Doc 17 §4.2: licensing_audit_log -- immutable log of signup, trial-start,
// validation, and billing events. Every mutating operation in this backend
// writes here; nothing here is ever updated or deleted in application code.
import type { Prisma, PrismaClient } from '@prisma/client';

export type AuditWriter = Pick<PrismaClient['licensingAuditLog'], 'create' | 'findMany'>;

export async function logAuditEvent(
  db: AuditWriter,
  params: {
    accountId?: string | null;
    eventType: string;
    deviceFingerprint?: string | null;
    payload?: Prisma.InputJsonValue;
  }
): Promise<void> {
  await db.create({
    data: {
      accountId: params.accountId ?? null,
      eventType: params.eventType,
      deviceFingerprint: params.deviceFingerprint ?? null,
      payload: params.payload ?? undefined,
    },
  });
}

/// Doc 30 TASK-LIC-002: "rate-limit activation attempts per key (e.g. 5/hour)
/// against brute-force key enumeration." Reuses licensing_audit_log rather
/// than a new table -- every attempt (success or failure) is logged as
/// 'activation_attempt' before the rate-limit check itself runs, so a
/// distributed brute-force pattern is visible in the same audit trail
/// TASK-LIC-009's fraud monitoring already reads.
export async function countRecentEvents(
  db: AuditWriter,
  eventType: string,
  windowMs: number,
  matchPayload: (payload: unknown) => boolean
): Promise<number> {
  const since = new Date(Date.now() - windowMs);
  const rows = await db.findMany({ where: { eventType, createdAt: { gte: since } } });
  return rows.filter((r) => matchPayload(r.payload)).length;
}
