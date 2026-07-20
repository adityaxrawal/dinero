import { useState, useEffect } from 'react';
import { Cpu, Loader2, Download, Trash2, CheckCircle, Server } from 'lucide-react';
import { API, LlmModelInfo } from '@/lib/ipc';
import { useIpcListen } from '@/hooks/useIpcListen';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

interface LlmDownloadProgress {
  model_id: string;
  bytes_downloaded: number;
  total_bytes: number | null;
}

export default function LocalLlmSettings() {
  const [availableModels, setAvailableModels] = useState<LlmModelInfo[]>([]);
  const [downloadedModels, setDownloadedModels] = useState<Set<string>>(new Set());
  const [activeModel, setActiveModel] = useState<string>('');
  const [downloads, setDownloads] = useState<Record<string, LlmDownloadProgress>>({});
  const [isDeleting, setIsDeleting] = useState<string | null>(null);

  useEffect(() => {
    Promise.all([
      API.llm.getAvailableModels(),
      API.llm.getDownloadedModels(),
      API.llm.getActiveModel(),
    ]).then(([models, downloaded, active]) => {
      setAvailableModels(models);
      setDownloadedModels(new Set(downloaded));
      setActiveModel(active);
    }).catch(err => console.error('Failed to load LLM state:', err));
  }, []);

  useIpcListen<LlmDownloadProgress>('llm_download_progress', (progress) => {
    setDownloads(prev => ({ ...prev, [progress.model_id]: progress }));
    if (progress.total_bytes && progress.bytes_downloaded >= progress.total_bytes) {
      setTimeout(() => {
        API.llm.getDownloadedModels().then(d => {
          setDownloadedModels(new Set(d));
          setDownloads(prev => {
            const next = { ...prev };
            delete next[progress.model_id];
            return next;
          });
        });
      }, 500);
    }
  });

  const handleDownload = async (modelId: string) => {
    try {
      setDownloads(prev => ({
        ...prev,
        [modelId]: { model_id: modelId, bytes_downloaded: 0, total_bytes: null }
      }));
      await API.llm.downloadModel(modelId);
      
      const updated = await API.llm.getDownloadedModels();
      setDownloadedModels(new Set(updated));
      setDownloads(prev => {
        const next = { ...prev };
        delete next[modelId];
        return next;
      });
    } catch (err) {
      alert(`Failed to download model: ${err}`);
      setDownloads(prev => {
        const next = { ...prev };
        delete next[modelId];
        return next;
      });
    }
  };

  const handleDelete = async (modelId: string) => {
    if (!confirm('Are you sure you want to delete this model?')) return;
    try {
      setIsDeleting(modelId);
      await API.llm.deleteModel(modelId);
      const updated = await API.llm.getDownloadedModels();
      setDownloadedModels(new Set(updated));
      
      if (modelId === activeModel) {
        // Fallback or leave it to the user to choose another
      }
    } catch (err) {
      alert(`Failed to delete model: ${err}`);
    } finally {
      setIsDeleting(null);
    }
  };

  const handleSetActive = async (modelInfo: LlmModelInfo) => {
    if (!downloadedModels.has(modelInfo.id)) {
      alert('You need to download this model first.');
      return;
    }
    
    try {
      const ramGb = await API.dev.checkSystemRam();
      const override = localStorage.getItem('llm_ram_override') === 'true';
      if (ramGb < modelInfo.min_ram_gb && !override) {
        if (!confirm(`Warning: Your system has ${ramGb.toFixed(1)}GB of RAM. ${modelInfo.name} requires at least ${modelInfo.min_ram_gb}GB. Allow override?`)) return;
        localStorage.setItem('llm_ram_override', 'true');
      }
    } catch (err) {
      console.error(err);
    }

    try {
      await API.llm.setActiveModel(modelInfo.id);
      setActiveModel(modelInfo.id);
    } catch (err) {
      alert(`Failed to set active model: ${err}`);
    }
  };

  const formatBytes = (bytes: number) => {
    return (bytes / 1073741824).toFixed(2) + ' GB';
  };

  return (
    <section>
      <div className="mb-6">
        <h2 className="text-xl font-bold flex items-center gap-2">
          <Cpu className="w-5 h-5" /> Local LLM Configuration
        </h2>
        <p className="text-sm mt-1 text-[#064E3B]/70">
          Select the local AI model for parsing statements. Heavier models perform better but require more RAM. Models must be downloaded before they can be selected.
        </p>
      </div>

      <div className="grid grid-cols-1 gap-4">
        {availableModels.map((m) => {
          const isDownloaded = downloadedModels.has(m.id);
          const isActive = activeModel === m.id;
          const dlProgress = downloads[m.id];
          const isDownloading = !!dlProgress;
          
          return (
            <div 
              key={m.id} 
              className={cn(
                "p-5 rounded-xl border flex flex-col sm:flex-row gap-5 items-start sm:items-center justify-between transition-colors",
                isActive 
                  ? "bg-[#064E3B]/5 border-[#064E3B]/30 shadow-sm" 
                  : "bg-[#F8E7C9]/50 border-[#064E3B]/10 hover:border-[#064E3B]/20"
              )}
            >
              <div className="flex-1 space-y-2">
                <div className="flex flex-wrap items-center gap-3">
                  <h3 className="font-bold text-[15px] text-[#064E3B]">{m.name}</h3>
                  {isActive && (
                    <span className="px-2.5 py-1 rounded-full bg-[#064E3B] text-[#F8E7C9] text-[11px] font-bold tracking-wide uppercase">
                      Active
                    </span>
                  )}
                  {isDownloaded && !isActive && (
                    <span className="px-2.5 py-1 rounded-full bg-emerald-500/10 text-emerald-700 border border-emerald-500/20 text-[11px] font-bold tracking-wide uppercase">
                      Downloaded
                    </span>
                  )}
                </div>
                
                <p className="text-[13px] text-[#064E3B]/70 leading-relaxed max-w-2xl">
                  {m.rationale}
                </p>
                
                <div className="flex flex-wrap items-center gap-4 text-[12px] font-medium text-[#064E3B]/60 pt-1">
                  <span className="flex items-center gap-1.5"><Server className="w-3.5 h-3.5"/> RAM: {m.min_ram_gb}GB+</span>
                  <span className="flex items-center gap-1.5">~{m.approx_size_gb}GB Size</span>
                  <span className="flex items-center gap-1.5">Tier {m.tier}</span>
                </div>

                {isDownloading && (
                  <div className="mt-3 w-full max-w-sm">
                    <div className="flex justify-between text-[11px] font-semibold text-[#064E3B]/60 mb-1.5">
                      <span>Downloading...</span>
                      {dlProgress.total_bytes ? (
                        <span>{formatBytes(dlProgress.bytes_downloaded)} / {formatBytes(dlProgress.total_bytes)}</span>
                      ) : (
                        <span>{formatBytes(dlProgress.bytes_downloaded)}</span>
                      )}
                    </div>
                    <div className="w-full h-1.5 rounded-full overflow-hidden bg-[#064E3B]/10">
                      <div
                        className="h-full bg-[#064E3B] transition-all duration-300"
                        style={{ 
                          width: dlProgress.total_bytes 
                            ? `${(dlProgress.bytes_downloaded / dlProgress.total_bytes) * 100}%` 
                            : '100%' 
                        }}
                      />
                    </div>
                  </div>
                )}
              </div>

              <div className="flex items-center gap-3 w-full sm:w-auto shrink-0 mt-2 sm:mt-0">
                {!isDownloaded && !isDownloading && (
                  <Button 
                    variant="outline"
                    className="flex-1 sm:flex-none h-9 font-semibold border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5" 
                    onClick={() => handleDownload(m.id)}
                  >
                    <Download className="w-4 h-4 mr-2" /> Download
                  </Button>
                )}
                
                {isDownloading && (
                  <Button 
                    variant="outline"
                    disabled
                    className="flex-1 sm:flex-none h-9 font-semibold border-[#064E3B]/20 text-[#064E3B]/50" 
                  >
                    <Loader2 className="w-4 h-4 mr-2 animate-spin" /> Downloading
                  </Button>
                )}

                {isDownloaded && !isActive && (
                  <Button 
                    className="flex-1 sm:flex-none h-9 font-semibold bg-[#064E3B] text-[#F8E7C9] hover:bg-[#064E3B]/90 shadow-sm" 
                    onClick={() => handleSetActive(m)}
                  >
                    <CheckCircle className="w-4 h-4 mr-2" /> Set Active
                  </Button>
                )}

                {isDownloaded && (
                  <Button 
                    variant="outline"
                    className="h-9 px-3 border-red-200 text-red-600 hover:text-red-700 hover:bg-red-50 hover:border-red-300"
                    onClick={() => handleDelete(m.id)}
                    disabled={isDeleting === m.id}
                    title="Delete model"
                  >
                    {isDeleting === m.id ? <Loader2 className="w-4 h-4 animate-spin" /> : <Trash2 className="w-4 h-4" />}
                  </Button>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
}
