/**
 * Rough wall-clock estimate for the *idle* state, before any real rate exists.
 * Assumes ~3s per transaction spread over the sidecar's ~6 concurrent slots —
 * deliberately coarse, since the real rate depends on the chosen model and the
 * Mac. Once a run starts, the measured rate replaces this everywhere.
 */
export function estimateMinutes(count: number): string {
  const minutes = Math.ceil((count * 3) / 6 / 60);
  if (minutes < 1) return 'under a minute';
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

/** `m:ss` under an hour, then `h:mm:ss`. */
export function formatClock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const s = total % 60;
  const m = Math.floor(total / 60) % 60;
  const h = Math.floor(total / 3600);
  const pad = (n: number) => String(n).padStart(2, '0');
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

export function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return '—';
  if (seconds < 60) return `${Math.ceil(seconds)}s`;
  const minutes = Math.ceil(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

export function formatAmount(amount: number | null, currency: string | null): string | null {
  if (amount === null) return null;
  try {
    return new Intl.NumberFormat(undefined, {
      style: 'currency',
      currency: currency ?? 'INR',
      maximumFractionDigits: 2,
    }).format(amount);
  } catch {
    return `${currency ?? ''} ${amount.toFixed(2)}`.trim();
  }
}

/** One line in the live feed of what the run just did. */
export type FeedEntry = {
  key: number;
  before: string;
  after: string | null;
  category: string | null;
};

export const FEED_LENGTH = 8;
