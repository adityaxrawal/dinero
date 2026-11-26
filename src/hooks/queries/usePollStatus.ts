/**
 * When Gmail was last checked and when the next background check is due
 * (audit F5-6). Refreshed every 30 s so the relative times stay current.
 */
import { useQuery } from '@tanstack/react-query';
import { API, type PollStatus } from '@/lib/ipc';
import { formatLastSynced } from '@/components/dashboard/formatLastSynced';

export function usePollStatus() {
  return useQuery({
    queryKey: ['pollStatus'],
    queryFn: API.ingestion.pollStatus,
    refetchInterval: 30_000,
  });
}

/** "Last checked 2 mins ago · next check in about 1 min", from real poll times. */
export function pollStatusText(status: PollStatus | undefined, now: Date = new Date()): string {
  if (!status) return 'Checking status…';
  if (status.paused) return 'Background checks paused';
  const last = status.last_checked_at
    ? `Last checked ${formatLastSynced(new Date(status.last_checked_at), now)}`
    : 'Not checked yet this session';
  const failed = status.last_check_failed ? ' · last check failed' : '';
  const nextMs = status.next_check_at ? new Date(status.next_check_at).getTime() - now.getTime() : null;
  const next =
    nextMs === null
      ? ''
      : nextMs <= 30_000
        ? ' · next check soon'
        : ` · next check in about ${Math.round(nextMs / 60_000) || 1} min`;
  return `${last}${failed}${next}`;
}
