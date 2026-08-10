/**
 * One downloadable model: size, requirements, and download state.
 */
import { CheckCircle, Download, Loader2, Server, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import type { LlmModelInfo } from '@/lib/ipc';
import { downloadDetail, downloadPercent, type LlmDownloadProgress } from './format';
import type { useLlmModels } from './useLlmModels';

type Models = ReturnType<typeof useLlmModels>;

const BADGE = 'px-2.5 py-1 rounded-full text-[11px] font-bold tracking-wide uppercase';
const DANGER_BUTTON =
  'h-9 px-3 border-red-200 text-red-600 hover:text-red-700 hover:bg-red-50 hover:border-red-300';

/** Size and requirement badges for a model. */
function Badges({
  isActive,
  isDownloaded,
  isRecommended,
}: {
  isActive: boolean;
  isDownloaded: boolean;
  isRecommended: boolean;
}) {
  return (
    <>
      {isActive && <span className={cn(BADGE, 'bg-[#064E3B] text-[#F8E7C9]')}>Active</span>}
      {isDownloaded && !isActive && (
        <span className={cn(BADGE, 'bg-emerald-500/10 text-emerald-700 border border-emerald-500/20')}>
          Downloaded
        </span>
      )}
      {isRecommended && (
        <span className={cn(BADGE, 'bg-blue-500/10 text-blue-700 border border-blue-500/20')}>
          Recommended for your Mac
        </span>
      )}
    </>
  );
}

/** Progress, speed and ETA for a downloading model. */
function DownloadProgress({ progress }: { progress: LlmDownloadProgress }) {
  const pct = downloadPercent(progress);
  return (
    <div className="mt-3 w-full max-w-sm">
      <div className="flex justify-between text-[11px] font-semibold text-[#064E3B]/60 mb-1.5">
        <span>Downloading...</span>
        {pct !== null && <span>{Math.round(pct)}%</span>}
      </div>
      <div className="w-full h-1.5 rounded-full overflow-hidden bg-[#064E3B]/10">
        <div
          className="h-full bg-[#064E3B] transition-all duration-300"
          style={{ width: pct !== null ? `${pct}%` : '100%' }}
        />
      </div>
      <div className="mt-1.5 text-[11px] text-[#064E3B]/60">{downloadDetail(progress)}</div>
    </div>
  );
}

/** Download, cancel, activate and delete actions. */
function ModelActions({
  model,
  models,
  isDownloaded,
  isActive,
  isDownloading,
}: {
  model: LlmModelInfo;
  models: Models;
  isDownloaded: boolean;
  isActive: boolean;
  isDownloading: boolean;
}) {
  return (
    <div className="flex items-center gap-3 w-full sm:w-auto shrink-0 mt-2 sm:mt-0">
      {!isDownloaded && !isDownloading && (
        <Button
          variant="outline"
          className="flex-1 sm:flex-none h-9 font-semibold border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5"
          onClick={() => models.download(model.id)}
        >
          <Download className="w-4 h-4 mr-2" /> Download
        </Button>
      )}

      {isDownloading && (
        <>
          <Button
            variant="outline"
            disabled
            className="flex-1 sm:flex-none h-9 font-semibold border-[#064E3B]/20 text-[#064E3B]/50"
          >
            <Loader2 className="w-4 h-4 mr-2 animate-spin" /> Downloading
          </Button>
          <Button
            variant="outline"
            className={DANGER_BUTTON}
            onClick={() => models.cancelDownload(model.id)}
            disabled={models.cancellingModelId === model.id}
            title="Cancel download"
          >
            Cancel
          </Button>
        </>
      )}

      {isDownloaded && !isActive && (
        <Button
          className="flex-1 sm:flex-none h-9 font-semibold bg-[#064E3B] text-[#F8E7C9] hover:bg-[#064E3B]/90 shadow-sm"
          onClick={() => models.setActive(model)}
        >
          <CheckCircle className="w-4 h-4 mr-2" /> Set Active
        </Button>
      )}

      {isDownloaded && (
        <Button
          variant="outline"
          className={DANGER_BUTTON}
          onClick={() => models.remove(model.id)}
          disabled={models.isDeleting === model.id}
          title="Delete model"
        >
          {models.isDeleting === model.id ? (
            <Loader2 className="w-4 h-4 animate-spin" />
          ) : (
            <Trash2 className="w-4 h-4" />
          )}
        </Button>
      )}
    </div>
  );
}

/** One model with its state and available actions. */
export default function ModelCard({ model, models }: { model: LlmModelInfo; models: Models }) {
  const isDownloaded = models.downloadedModels.has(model.id);
  const isActive = models.activeModel === model.id;
  const progress = models.downloads[model.id];

  return (
    <div
      className={cn(
        'p-5 rounded-xl border flex flex-col sm:flex-row gap-5 items-start sm:items-center justify-between transition-colors',
        isActive
          ? 'bg-[#064E3B]/5 border-[#064E3B]/30 shadow-sm'
          : 'bg-[#F8E7C9]/50 border-[#064E3B]/10 hover:border-[#064E3B]/20'
      )}
    >
      <div className="flex-1 space-y-2">
        <div className="flex flex-wrap items-center gap-3">
          <h3 className="font-bold text-[15px] text-[#064E3B]">{model.name}</h3>
          <Badges
            isActive={isActive}
            isDownloaded={isDownloaded}
            isRecommended={models.hwInfo?.recommended_model_id === model.id}
          />
        </div>

        <p className="text-[13px] text-[#064E3B]/70 leading-relaxed max-w-2xl">{model.rationale}</p>

        <div className="flex flex-wrap items-center gap-4 text-[12px] font-medium text-[#064E3B]/60 pt-1">
          <span className="flex items-center gap-1.5">
            <Server className="w-3.5 h-3.5" /> RAM: {model.min_ram_gb}GB+
          </span>
          <span className="flex items-center gap-1.5">~{model.approx_size_gb}GB Size</span>
          <span className="flex items-center gap-1.5">Tier {model.tier}</span>
        </div>

        {progress && <DownloadProgress progress={progress} />}
      </div>

      <ModelActions
        model={model}
        models={models}
        isDownloaded={isDownloaded}
        isActive={isActive}
        isDownloading={!!progress}
      />
    </div>
  );
}
