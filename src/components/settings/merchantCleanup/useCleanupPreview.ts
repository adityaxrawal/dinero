import { useState, useEffect, useCallback } from 'react';
import { API, type MerchantCleanupPreview, type MerchantCleanupRun, type LlmModelInfo } from '@/lib/ipc';
import { errorMessage } from '@/lib/utils';

/** The queue and the run log, both derived server-side from confidence. */
export function useCleanupPreview() {
  const [preview, setPreview] = useState<MerchantCleanupPreview | null>(null);
  const [runs, setRuns] = useState<MerchantCleanupRun[]>([]);
  const [activeModel, setActiveModel] = useState<LlmModelInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  const loadPreview = useCallback(() => {
    API.merchantCleanup
      .preview()
      .then(setPreview)
      .catch((err) => setError(errorMessage(err)));
  }, []);

  const loadRuns = useCallback(() => {
    API.merchantCleanup
      .runs()
      .then(setRuns)
      .catch((err) => setError(errorMessage(err)));
  }, []);

  useEffect(() => {
    loadPreview();
    loadRuns();
    // The model name is what makes "on-device AI" concrete; without it the user
    // cannot tell what is about to read their mail.
    Promise.all([API.llm.getActiveModel(), API.llm.getAvailableModels()])
      .then(([id, models]) => setActiveModel(models.find((m) => m.id === id) ?? null))
      .catch(() => setActiveModel(null));
  }, [loadPreview, loadRuns]);

  return { preview, runs, activeModel, error, setError, loadPreview, loadRuns };
}
