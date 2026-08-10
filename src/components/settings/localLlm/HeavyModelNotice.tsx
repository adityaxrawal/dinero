/**
 * Warns when the selected model exceeds what this machine can comfortably run.
 *
 * Advisory rather than blocking -- the user may proceed knowingly, since the
 * hardware estimate is a heuristic and not always right.
 */
import type { LlmHardwareInfo, LlmModelInfo } from '@/lib/ipc';

/** Warns when the chosen model exceeds this machine's comfortable capacity. */
export default function HeavyModelNotice({
  hwInfo,
  availableModels,
  activeModel,
}: {
  hwInfo: LlmHardwareInfo | null;
  availableModels: LlmModelInfo[];
  activeModel: string;
}) {
  if (!hwInfo || !activeModel) return null;

  const recommended = availableModels.find((m) => m.id === hwInfo.recommended_model_id);
  const selected = availableModels.find((m) => m.id === activeModel);
  if (!recommended || !selected || selected.tier <= recommended.tier) return null;

  return (
    <div className="mb-6 p-4 rounded-xl border border-amber-400/40 bg-amber-400/10 text-[13px] text-amber-900">
      <strong>{selected.name}</strong> is heavier than recommended for your hardware (
      {hwInfo.ram_gb.toFixed(0)}GB RAM). If extraction feels slow, consider switching to{' '}
      <strong>{recommended.name}</strong>.
    </div>
  );
}
