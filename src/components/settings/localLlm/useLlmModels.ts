import { useState, useEffect } from 'react';
import { API, type LlmModelInfo, type LlmHardwareInfo } from '@/lib/ipc';
import { useIpcListen } from '@/hooks/useIpcListen';
import { RAM_OVERRIDE_STORAGE_KEY, type LlmDownloadProgress } from './format';

/** Returns false only when the user declines the not-enough-RAM warning. */
async function ramAllows(model: LlmModelInfo): Promise<boolean> {
  try {
    const ramGb = await API.dev.checkSystemRam();
    if (localStorage.getItem(RAM_OVERRIDE_STORAGE_KEY) === 'true') return true;
    if (ramGb >= model.min_ram_gb) return true;

    const allow = confirm(
      `Warning: Your system has ${ramGb.toFixed(1)}GB of RAM. ${model.name} requires at least ${model.min_ram_gb}GB. Allow override?`
    );
    if (allow) localStorage.setItem(RAM_OVERRIDE_STORAGE_KEY, 'true');
    return allow;
  } catch (err) {
    // A RAM probe failure must not block the user from choosing a model.
    console.error(err);
    return true;
  }
}

export function useLlmModels(onHardware: (hw: LlmHardwareInfo) => number) {
  const [availableModels, setAvailableModels] = useState<LlmModelInfo[]>([]);
  const [downloadedModels, setDownloadedModels] = useState<Set<string>>(new Set());
  const [activeModel, setActiveModel] = useState<string>('');
  const [downloads, setDownloads] = useState<Record<string, LlmDownloadProgress>>({});
  const [isDeleting, setIsDeleting] = useState<string | null>(null);
  const [cancellingModelId, setCancellingModelId] = useState<string | null>(null);
  const [hwInfo, setHwInfo] = useState<LlmHardwareInfo | null>(null);

  const clearDownload = (modelId: string) =>
    setDownloads((prev) => {
      const next = { ...prev };
      delete next[modelId];
      return next;
    });

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

  const download = async (modelId: string) => {
    try {
      setDownloads((prev) => ({
        ...prev,
        [modelId]: { model_id: modelId, bytes_downloaded: 0, total_bytes: null, bytes_per_sec: 0 },
      }));
      await API.llm.downloadModel(modelId);
      await refreshDownloaded();
    } catch (err) {
      alert(`Failed to download model: ${err}`);
    } finally {
      clearDownload(modelId);
    }
  };

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

  const remove = async (modelId: string) => {
    if (!confirm('Are you sure you want to delete this model?')) return;
    try {
      setIsDeleting(modelId);
      const newActiveModel = await API.llm.deleteModel(modelId);
      await refreshDownloaded();
      setActiveModel(newActiveModel);
    } catch (err) {
      alert(`Failed to delete model: ${err}`);
    } finally {
      setIsDeleting(null);
    }
  };

  const setActive = async (model: LlmModelInfo) => {
    if (!downloadedModels.has(model.id)) {
      alert('You need to download this model first.');
      return;
    }
    if (!(await ramAllows(model))) return;
    try {
      await API.llm.setActiveModel(model.id);
      setActiveModel(model.id);
    } catch (err) {
      alert(`Failed to set active model: ${err}`);
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
