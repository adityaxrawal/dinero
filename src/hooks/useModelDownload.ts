/**
 * Live progress of a local-AI model download, and a way to start one.
 *
 * Shared by onboarding (which starts the download, owner decision Q-1) and the
 * sidebar banner (the fallback when the download was skipped or failed).
 */
import { useCallback, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { useIpcListen } from '@/hooks/useIpcListen';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';
import type { LlmDownloadProgress } from '@/components/settings/localLlm/format';

/** Percent 0-100, or null when the size is not known yet. */
export function downloadPercent(p: LlmDownloadProgress): number | null {
  return p.total_bytes ? Math.min(100, Math.round((p.bytes_downloaded / p.total_bytes) * 100)) : null;
}

/** Starts downloading the model the user chose (falls back to the active one). */
export async function startModelDownload(modelId?: string): Promise<void> {
  const id = modelId ?? (await API.llm.getActiveModel());
  await API.llm.setActiveModel(id);
  await API.llm.downloadModel(id);
}

export function useModelDownload() {
  const queryClient = useQueryClient();
  const [progress, setProgress] = useState<LlmDownloadProgress | null>(null);

  useIpcListen<LlmDownloadProgress>('llm_download_progress', (p) => {
    const done = p.total_bytes !== null && p.bytes_downloaded >= p.total_bytes;
    setProgress(done ? null : p);
    // The "waiting for the AI" line and banner read llm_state from here.
    if (done) queryClient.invalidateQueries({ queryKey: queryKeys.reconciliation.llmPending() });
  });

  const start = useCallback(
    (modelId?: string) =>
      startModelDownload(modelId).finally(() => {
        setProgress(null);
        queryClient.invalidateQueries({ queryKey: queryKeys.reconciliation.llmPending() });
      }),
    [queryClient]
  );

  return { progress, start };
}
