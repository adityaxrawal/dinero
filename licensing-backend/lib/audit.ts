/**
 * Append-only audit trail for licensing events.
 *
 * Every activation, deactivation and binding change is recorded here. The log
 * serves two purposes: it is the evidence trail for support investigations, and
 * it is the substrate the fraud heuristics count against -- which is why the
 * read helper below filters by time window rather than fetching everything.
 */
import type { Prisma, PrismaClient } from '@prisma/client';

export type AuditWriter = Pick<PrismaClient['licensingAuditLog'], 'create' | 'findMany'>;

/**
 * Record one event. Optional fields are normalised so a row never carries
 * undefined, keeping the stored shape consistent across event types.
 */
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

/**
 * Count events of a type within a window that satisfy a payload predicate.
 *
 * The predicate runs in application code rather than SQL because the payload is
 * an opaque JSON column with no queryable structure. That means the window rows
 * are loaded before filtering, so this stays cheap only while the window is
 * short -- it is sized for fraud checks over minutes, not analytics over months.
 *
 * ponytail: in-memory payload filter, push into a JSON query if windows grow
 */
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
