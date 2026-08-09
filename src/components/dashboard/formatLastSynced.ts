/**
 * TASK-RT-005 (Doc 30): "near-real-time," never "real-time" -- updates only
 * happen while the app is open/backgrounded (Gmail smart-polling,
 * `transaction_created` invalidation), never while asleep or fully quit, so
 * the copy must not imply instant push delivery. Kept as a plain relative
 * string ("just now" / "2 min ago") rather than reusing `formatRelativeDate`
 * (day-granularity only -- "Today"/"Yesterday" -- useless for a timestamp
 * that updates within the same session).
 */
export function formatLastSynced(syncedAt: Date, now: Date = new Date()): string {
  const seconds = Math.max(0, Math.floor((now.getTime() - syncedAt.getTime()) / 1000));
  if (seconds < 30) return 'just now';
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min${minutes === 1 ? '' : 's'} ago`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ago`;
}
