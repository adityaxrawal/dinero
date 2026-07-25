export function formatDuration(totalSeconds: number): string {
  const seconds = Math.max(0, Math.round(totalSeconds));
  if (seconds < 60) return `${seconds}s`;
  const totalMinutes = Math.floor(seconds / 60);
  const remainingSeconds = seconds % 60;
  if (totalMinutes < 60) return `${totalMinutes}m ${remainingSeconds}s`;
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  return `${hours}h ${minutes}m`;
}

export function estimateEtaSeconds(
  processed: number,
  total: number,
  elapsedSeconds: number
): number | null {
  if (processed <= 0 || total <= 0 || processed >= total) return null;
  const secondsPerItem = elapsedSeconds / processed;
  return Math.round(secondsPerItem * (total - processed));
}
