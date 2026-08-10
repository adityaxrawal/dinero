/**
 * Formats the last successful sync time as relative text.
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
