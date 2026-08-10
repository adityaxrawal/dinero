/**
 * Trial conversion funnel, reconstructed from the audit log.
 *
 * Derived from events rather than subscription state because the funnel is
 * inherently historical: it must count trials that expired without converting,
 * which by definition leave no active row behind. Each stage corresponds to one
 * audit event type, listed together below so the funnel's shape is visible in
 * one place.
 */
import type { AuditWriter } from './audit';

export interface ConversionFunnelSummary {
  trials_started: number;
  day10_reminders_sent: number;
  day13_reminders_sent: number;
  converted: number;
  expired_unconverted: number;
}

const FUNNEL_EVENT_TYPES = {
  started: 'trial_started',
  day10: 'trial_day10_reminder',
  day13: 'trial_day13_reminder',
  converted: 'trial_converted',
  expired: 'trial_expired_unconverted',
} as const;

/**
 * Reconstructs the trial funnel from audit events.
 *
 * Derived from events rather than subscription state because the funnel must
 * count trials that expired without converting, which by definition leave no
 * active row behind.
 */
export async function computeConversionFunnel(
  db: AuditWriter,
  windowDays: number
): Promise<ConversionFunnelSummary> {
  const windowMs = windowDays * 24 * 60 * 60 * 1000;
  const since = new Date(Date.now() - windowMs);

  const counts = await Promise.all(
    Object.values(FUNNEL_EVENT_TYPES).map((eventType) =>
      db.findMany({ where: { eventType, createdAt: { gte: since } } }).then((rows) => rows.length)
    )
  );

  return {
    trials_started: counts[0],
    day10_reminders_sent: counts[1],
    day13_reminders_sent: counts[2],
    converted: counts[3],
    expired_unconverted: counts[4],
  };
}
