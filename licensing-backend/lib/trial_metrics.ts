// Doc 30 TASK-LIC-007: conversion-funnel metrics feeding the weekly review
// against Doc 01 BG-05's >=30%-within-90-days target. Aggregate-only --
// never per-user breakdowns (minimal-PII posture, Doc 30 TASK-BILL-007).
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
