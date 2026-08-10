/**
 * Folds an incoming progress update into the task record already on screen.
 *
 * Backend progress events are partial by nature: a scan emits frequent updates
 * carrying only a counter, and a completion event may carry only a status. A
 * naive object merge would therefore blank out fields the event simply did not
 * mention. Every field below is resolved as "take the incoming value if it was
 * supplied, otherwise keep what we already had", which is what lets a task
 * accumulate detail across many partial events without ever losing it.
 *
 * `now` is passed in rather than read from the clock so merges are deterministic
 * and testable.
 */
import type { NotificationCategory, TaskStatus, UnifiedTask } from './useNotificationStore';

/** A partial update; only identity, title and category are always present. */
export type TaskUpdate = Partial<UnifiedTask> & {
  id: string;
  title: string;
  category: NotificationCategory;
};

/**
 * First value that is neither null nor undefined.
 *
 * The `!= null` test is intentional -- it rejects both null and undefined while
 * still accepting 0 and empty string, which are legitimate progress values that
 * a plain falsy check would incorrectly discard.
 */
function firstDefined<T>(...values: (T | null | undefined)[]): T | undefined {
  return values.find((v) => v != null) ?? undefined;
}

/**
 * Compute a percentage from current/total, falling back when total is unknown.
 *
 * Total is zero while a task is still enumerating its work, so the previous
 * percentage is retained instead of snapping the bar back to zero. The result is
 * capped at 100 because a backend counter can briefly overshoot its own total.
 */
function derivePercent(current: number, total: number, fallback: number | undefined): number {
  if (total > 0) return Math.min(100, Math.round((current / total) * 100));
  return fallback ?? 0;
}

/**
 * Resolve the ETA, treating null and undefined as different answers.
 *
 * An explicit null means "there is no longer an estimate" and must overwrite a
 * previous value, whereas undefined means the event said nothing about the ETA
 * and the existing one should stand. This is why the check is `!== undefined`
 * rather than a nullish coalesce.
 */
function mergeEta(incoming: TaskUpdate, prev: Partial<UnifiedTask>): number | null | undefined {
  if (incoming.etaSeconds !== undefined) return incoming.etaSeconds;
  return prev.etaSeconds ?? null;
}

/**
 * Stamp a completion time, inferring one when the backend omits it.
 *
 * Any status other than 'running' -- success, failure, cancellation -- means the
 * task has ended, so a transition into one of those states without an explicit
 * timestamp is dated to now. Without this, finished tasks would keep a null
 * finishedAt and could never be sorted or aged out of the notification list.
 */
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
 * Produce the next task record from the previous one plus a partial update.
 *
 * Pure and total: an absent `existing` is treated as an empty object, so the
 * same function handles both the first event for a task and every subsequent
 * one, and a brand-new task needs no separate construction path.
 */
export function mergeTask(
  existing: UnifiedTask | undefined,
  incoming: TaskUpdate,
  now: number
): UnifiedTask {
  const prev: Partial<UnifiedTask> = existing ?? {};

  // Resolved before the return because derivePercent needs both values.
  const current = firstDefined(incoming.current, prev.current) ?? 0;
  const total = firstDefined(incoming.total, prev.total) ?? 0;

  return {
    // Identity and presentation always come from the incoming event -- these
    // three are required on every update, so there is nothing to fall back to.
    id: incoming.id,
    // Groups related tasks under one heading; defaults to the id, which makes
    // an ungrouped task its own group.
    domainKey: firstDefined(incoming.domainKey, prev.domainKey) ?? incoming.id,
    category: incoming.category,
    title: incoming.title,
    description: firstDefined(incoming.description, prev.description) ?? '',
    status: firstDefined(incoming.status, prev.status) ?? ('running' as TaskStatus),
    current,
    total,
    // An explicit percentage from the backend wins; otherwise it is derived
    // from the counters.
    progressPct: incoming.progressPct ?? derivePercent(current, total, prev.progressPct),
    etaSeconds: mergeEta(incoming, prev),

    // Set once on the first event and never overwritten, so elapsed time is
    // measured from when the task actually began.
    startedAt: prev.startedAt ?? now,
    updatedAt: now,
    finishedAt: mergeFinishedAt(incoming, prev, now),

    errorMessage: firstDefined(incoming.errorMessage, prev.errorMessage) ?? null,
    // Defaults to false: a task is only cancellable if it says so.
    cancelable: firstDefined(incoming.cancelable, prev.cancelable) ?? false,

    // Meta is the one field that accumulates rather than replaces -- events
    // contribute different keys over a task's life, and all of them are kept.
    meta: { ...(prev.meta ?? {}), ...(incoming.meta ?? {}) },
  };
}
