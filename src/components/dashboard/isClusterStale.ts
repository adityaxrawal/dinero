/**
 * Decides whether a cluster has gone unresolved long enough to prompt about.
 */
const STALE_THRESHOLD_MS = 7 * 24 * 60 * 60 * 1000;

/** Whether a cluster has gone unresolved long enough to prompt about. */
export function isClusterStale(createdAt: string | null, now: Date = new Date()): boolean {
  if (!createdAt) return false;
  const hasTimezone = /[zZ]$|[+-]\d{2}:?\d{2}$/.test(createdAt);
  const iso = hasTimezone ? createdAt : `${createdAt.replace(' ', 'T')}Z`;
  const created = new Date(iso);
  if (Number.isNaN(created.getTime())) return false;
  return now.getTime() - created.getTime() >= STALE_THRESHOLD_MS;
}
