export const PARALLEL_SLOTS_STORAGE_KEY = 'llm_parallel_slots';
export const RAM_OVERRIDE_STORAGE_KEY = 'llm_ram_override';

export const clampSlots = (n: number) => {
  if (!Number.isFinite(n)) return 1;
  return Math.min(10, Math.max(1, Math.round(n)));
};

export function storedSlots(): number | null {
  const stored = localStorage.getItem(PARALLEL_SLOTS_STORAGE_KEY);
  return stored ? clampSlots(parseInt(stored, 10)) : null;
}

function formatBytes(bytes: number): string {
  return (bytes / 1073741824).toFixed(2) + ' GB';
}

function formatSpeed(bytesPerSec: number): string | null {
  if (bytesPerSec <= 0) return null;
  if (bytesPerSec < 1_048_576) return `${(bytesPerSec / 1024).toFixed(0)} KB/s`;
  return `${(bytesPerSec / 1_048_576).toFixed(1)} MB/s`;
}

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

/** The "1.2 GB / 4.0 GB · 3.1 MB/s · ~2m left" line under the progress bar. */
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

export function downloadPercent(progress: LlmDownloadProgress): number | null {
  return progress.total_bytes ? (progress.bytes_downloaded / progress.total_bytes) * 100 : null;
}
