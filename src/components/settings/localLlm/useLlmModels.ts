/**
 * Model catalogue state: available models, download progress, activation.
 */
import { useState, useEffect } from 'react';
import { API, type LlmModelInfo, type LlmHardwareInfo } from '@/lib/ipc';
import { useIpcListen } from '@/hooks/useIpcListen';
import { toast } from '@/hooks/use-toast';
import { confirmAction } from '@/lib/confirmDialog';
import type { LlmDownloadProgress } from './format';

/**
 * Whether this machine has enough RAM for a model, honouring a prior override.
 *
 * A user who previously accepted the risk is not asked again -- the override is
 * remembered, so the warning appears once per machine rather than per attempt.
 */
async function ramAllows(model: LlmModelInfo): Promise<boolean> {
  try {
    const hw = await API.llm.getHardwareInfo();
    if (hw.ram_override || hw.ram_gb >= model.min_ram_gb) return true;

    const allow = await confirmAction(
      `This Mac has ${hw.ram_gb.toFixed(1)} GB of RAM. ${model.name} needs at least ${model.min_ram_gb} GB, so it may run slowly or crash. Use it anyway?`,
      'Not enough memory'
    );
    if (allow) await API.llm.setRamOverride(true);
    return allow;
  } catch (err) {
    console.error(err);
    return true;
  }
}

/**
 * Model catalogue state: what is available, downloaded, and active.
 *
 * Takes a hardware callback rather than reading hardware itself, so the caller
 * decides how the RAM figure is obtained and this hook stays testable.
 */
export function useLlmModels(onHardware: (hw: LlmHardwareInfo) => number) {
  const [availableModels, setAvailableModels] = useState<LlmModelInfo[]>([]);
  const [downloadedModels, setDownloadedModels] = useState<Set<string>>(new Set());
  const [activeModel, setActiveModel] = useState<string>('');
  const [downloads, setDownloads] = useState<Record<string, LlmDownloadProgress>>({});
  const [isDeleting, setIsDeleting] = useState<string | null>(null);
  const [cancellingModelId, setCancellingModelId] = useState<string | null>(null);
  const [hwInfo, setHwInfo] = useState<LlmHardwareInfo | null>(null);

  /** Clears a finished or cancelled download's progress state. */
  const clearDownload = (modelId: string) =>
    setDownloads((prev) => {
      const next = { ...prev };
      delete next[modelId];
      return next;
    });

  /** Re-reads which models are present on disk. */
  const refreshDownloaded = async () => {
    setDownloadedModels(new Set(await API.llm.getDownloadedModels()));
  };

  useEffect(() => {
    Promise.all([
      API.llm.getAvailableModels(),
      API.llm.getDownloadedModels(),
      API.llm.getActiveModel(),
      API.llm.getHardwareInfo(),
    ])
      .then(([models, downloaded, active, hw]) => {
        setAvailableModels(models);
        setDownloadedModels(new Set(downloaded));
        setActiveModel(active);
        setHwInfo(hw);
        const initial = onHardware(hw);
        API.llm
          .setParallelSlots(initial)
          .catch((err) => console.error('Failed to sync parallel slots:', err));
      })
      .catch((err) => console.error('Failed to load LLM state:', err));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useIpcListen<LlmDownloadProgress>('llm_download_progress', (progress) => {
    setDownloads((prev) => ({ ...prev, [progress.model_id]: progress }));
    if (progress.total_bytes && progress.bytes_downloaded >= progress.total_bytes) {
      setTimeout(() => {
        API.llm.getDownloadedModels().then((d) => {
          setDownloadedModels(new Set(d));
          clearDownload(progress.model_id);
        });
      }, 500);
    }
  });

  /** Starts a download after checking RAM suitability. */
  const download = async (modelId: string) => {
    try {
      setDownloads((prev) => ({
        ...prev,
        [modelId]: { model_id: modelId, bytes_downloaded: 0, total_bytes: null, bytes_per_sec: 0 },
      }));
      await API.llm.downloadModel(modelId);
      await refreshDownloaded();
    } catch (err) {
      toast({ title: 'Download failed', description: String(err), variant: 'destructive' });
    } finally {
      clearDownload(modelId);
    }
  };

  /** Cancels an in-flight download. */
  const cancelDownload = async (modelId: string) => {
    setCancellingModelId(modelId);
    try {
      await API.llm.cancelDownload(modelId);
    } catch (err) {
      console.error('Failed to cancel download:', err);
    } finally {
      setCancellingModelId(null);
    }
  };

  /** Deletes a downloaded model. */
  const remove = async (modelId: string) => {
    if (!(await confirmAction('Delete this model? You can download it again later.', 'Delete Model')))
      return;
    try {
      setIsDeleting(modelId);
      const newActiveModel = await API.llm.deleteModel(modelId);
      await refreshDownloaded();
      setActiveModel(newActiveModel);
    } catch (err) {
      toast({ title: 'Failed to delete model', description: String(err), variant: 'destructive' });
    } finally {
      setIsDeleting(null);
    }
  };

  /** Activates a model for inference. */
  const setActive = async (model: LlmModelInfo) => {
    if (!downloadedModels.has(model.id)) {
      toast({ title: 'You need to download this model first.' });
      return;
    }
    if (!(await ramAllows(model))) return;
    try {
      await API.llm.setActiveModel(model.id);
      setActiveModel(model.id);
    } catch (err) {
      toast({
        title: 'Failed to set active model',
        description: String(err),
        variant: 'destructive',
      });
    }
  };

  return {
    availableModels,
    downloadedModels,
    activeModel,
    downloads,
    isDeleting,
    cancellingModelId,
    hwInfo,
    download,
    cancelDownload,
    remove,
    setActive,
  };
}
