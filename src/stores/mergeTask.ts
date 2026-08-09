import type { NotificationCategory, TaskStatus, UnifiedTask } from './useNotificationStore';

export type TaskUpdate = Partial<UnifiedTask> & {
  id: string;
  title: string;
  category: NotificationCategory;
};

/** First value that is neither null nor undefined — one `??` chain, one branch. */
function firstDefined<T>(...values: (T | null | undefined)[]): T | undefined {
  return values.find((v) => v != null) ?? undefined;
}

function derivePercent(current: number, total: number, fallback: number | undefined): number {
  if (total > 0) return Math.min(100, Math.round((current / total) * 100));
  return fallback ?? 0;
}

/** An incoming `null` is a real "ETA no longer known" and must overwrite the
 *  previous estimate, so this is `!== undefined` rather than `??`. */
function mergeEta(incoming: TaskUpdate, prev: Partial<UnifiedTask>): number | null | undefined {
  if (incoming.etaSeconds !== undefined) return incoming.etaSeconds;
  return prev.etaSeconds ?? null;
}

function mergeFinishedAt(
  incoming: TaskUpdate,
  prev: Partial<UnifiedTask>,
  now: number
): number | null | undefined {
  if (incoming.finishedAt != null) return incoming.finishedAt;
  const isFinishing = Boolean(incoming.status && incoming.status !== 'running');
  return isFinishing ? now : prev.finishedAt;
}

/**
 * Folds a progress event onto whatever is already known about the task.
 * Every field falls back to the existing task before its own default, so a
 * partial event never blanks out detail an earlier one supplied.
 */
export function mergeTask(
  existing: UnifiedTask | undefined,
  incoming: TaskUpdate,
  now: number
): UnifiedTask {
  // Destructured once so the 12 field reads below need no `existing?.` guard.
  const prev: Partial<UnifiedTask> = existing ?? {};
  const current = firstDefined(incoming.current, prev.current) ?? 0;
  const total = firstDefined(incoming.total, prev.total) ?? 0;

  return {
    id: incoming.id,
    domainKey: firstDefined(incoming.domainKey, prev.domainKey) ?? incoming.id,
    category: incoming.category,
    title: incoming.title,
    description: firstDefined(incoming.description, prev.description) ?? '',
    status: firstDefined(incoming.status, prev.status) ?? ('running' as TaskStatus),
    current,
    total,
    progressPct: incoming.progressPct ?? derivePercent(current, total, prev.progressPct),
    etaSeconds: mergeEta(incoming, prev),
    startedAt: prev.startedAt ?? now,
    updatedAt: now,
    finishedAt: mergeFinishedAt(incoming, prev, now),
    errorMessage: firstDefined(incoming.errorMessage, prev.errorMessage) ?? null,
    cancelable: firstDefined(incoming.cancelable, prev.cancelable) ?? false,
    meta: { ...(prev.meta ?? {}), ...(incoming.meta ?? {}) },
  };
}
