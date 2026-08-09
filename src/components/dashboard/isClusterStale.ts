const STALE_THRESHOLD_MS = 7 * 24 * 60 * 60 * 1000;

/**
 * TASK-RT-006 (Doc 30): "a subtle periodic reminder ... if unresolved
 * clusters have existed more than 7 days, respecting that leaving a
 * cluster unresolved is a legitimate, deliberate user choice." SQLite's
 * `CURRENT_TIMESTAMP` format (`YYYY-MM-DD HH:MM:SS`, no offset) is always
 * UTC -- normalized to an ISO string here so this doesn't silently depend
 * on the browser parsing a bare space-separated datetime as local time.
 */
export function isClusterStale(createdAt: string | null, now: Date = new Date()): boolean {
  if (!createdAt) return false;
  const hasTimezone = /[zZ]$|[+-]\d{2}:?\d{2}$/.test(createdAt);
  const iso = hasTimezone ? createdAt : `${createdAt.replace(' ', 'T')}Z`;
  const created = new Date(iso);
  if (Number.isNaN(created.getTime())) return false;
  return now.getTime() - created.getTime() >= STALE_THRESHOLD_MS;
}
