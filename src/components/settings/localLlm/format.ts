/**
 * Formatting helpers and storage keys shared by the local-LLM settings.
 *
 * The storage keys are exported so the startup sync path and this UI agree on
 * where preferences live; a mismatch would silently split them in two.
 */
export const PARALLEL_SLOTS_STORAGE_KEY = 'llm_parallel_slots';
export const RAM_OVERRIDE_STORAGE_KEY = 'llm_ram_override';

/**
 * Bounds a slot count to the range the UI can represent.
 *
 * The finite check catches a NaN from parsing corrupted storage, which would
 * otherwise propagate into the backend as an invalid concurrency setting.
 */
export const clampSlots = (n: number) => {
  if (!Number.isFinite(n)) return 1;
  return Math.min(10, Math.max(1, Math.round(n)));
};

/**
 * Reads the saved slot preference, or null if none was stored.
 *
 * The stored value is re-clamped on read rather than trusted, since it may have
 * been written by an older build with different bounds.
 */
export function storedSlots(): number | null {
  const stored = localStorage.getItem(PARALLEL_SLOTS_STORAGE_KEY);
  return stored ? clampSlots(parseInt(stored, 10)) : null;
}

/** Renders a byte count in GB -- models are always gigabyte-scale. */
function formatBytes(bytes: number): string {
  return (bytes / 1073741824).toFixed(2) + ' GB';
}

/**
 * Renders transfer speed, switching unit at the megabyte boundary.
 *
 * Returns null for a non-positive rate so the caller can omit the field rather
 * than display a meaningless "0 KB/s" at the start of a download.
 */
function formatSpeed(bytesPerSec: number): string | null {
  if (bytesPerSec <= 0) return null;
  if (bytesPerSec < 1_048_576) return `${(bytesPerSec / 1024).toFixed(0)} KB/s`;
  return `${(bytesPerSec / 1_048_576).toFixed(1)} MB/s`;
}

/**
 * Renders a remaining-time estimate at decreasing precision as it grows.
 *
 * Rounds up throughout, since an estimate that finishes early reads better than
 * one that overruns.
 */
function formatDuration(seconds: number): string {
  if (seconds < 60) return `${Math.ceil(seconds)}s`;
  const totalMinutes = Math.ceil(seconds / 60);
  if (totalMinutes < 60) return `${totalMinutes}m`;
  const hours = Math.floor(totalMinutes / 60);
  return `${hours}h ${totalMinutes % 60}m`;
}

export interface LlmDownloadProgress {
  model_id: string;
  bytes_downloaded: number;
  total_bytes: number | null;
  bytes_per_sec: number;
}

/**
 * Builds the one-line status under a downloading model.
 *
 * Assembled from whichever parts are meaningful: total size is unknown until the
 * server reports it, and speed and ETA are omitted before transfer begins. The
 * pieces are joined only after filtering, so the separator never dangles.
 */
export function downloadDetail(progress: LlmDownloadProgress): string {
  const speed = formatSpeed(progress.bytes_per_sec);
  const remaining = progress.total_bytes ? progress.total_bytes - progress.bytes_downloaded : null;
  const eta =
    remaining !== null && progress.bytes_per_sec > 0
      ? formatDuration(remaining / progress.bytes_per_sec)
      : null;

  return [
    progress.total_bytes
      ? `${formatBytes(progress.bytes_downloaded)} / ${formatBytes(progress.total_bytes)}`
      : formatBytes(progress.bytes_downloaded),
    ...(speed ? [speed] : []),
    ...(eta ? [`~${eta} left`] : []),
  ].join(' · ');
}

/**
 * Percentage complete, or null when the total size is not yet known.
 *
 * Null lets the caller render an indeterminate bar rather than a stalled zero.
 */
export function downloadPercent(progress: LlmDownloadProgress): number | null {
  return progress.total_bytes ? (progress.bytes_downloaded / progress.total_bytes) * 100 : null;
}
