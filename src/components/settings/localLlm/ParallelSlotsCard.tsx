import { CheckCircle, Loader2, Server } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import type { LlmHardwareInfo } from '@/lib/ipc';
import { clampSlots } from './format';
import type { useParallelSlots } from './useParallelSlots';

type Slots = ReturnType<typeof useParallelSlots>;

function SaveButton({ slots }: { slots: Slots }) {
  const label = slots.isSaving ? 'Saving…' : slots.justSaved ? 'Saved' : 'Save';
  return (
    <Button
      variant="accent"
      size="sm"
      className="ml-auto"
      onClick={slots.save}
      disabled={!slots.isDirty || slots.isSaving}
    >
      {slots.isSaving ? (
        <Loader2 className="w-3.5 h-3.5 animate-spin" />
      ) : slots.justSaved ? (
        <CheckCircle className="w-3.5 h-3.5" />
      ) : null}
      {label}
    </Button>
  );
}

export default function ParallelSlotsCard({
  slots,
  hwInfo,
}: {
  slots: Slots;
  hwInfo: LlmHardwareInfo | null;
}) {
  const { isDirty, draftSlots } = slots;

  return (
    <div
      className={cn(
        'mb-6 p-5 rounded-xl border transition-colors',
        isDirty ? 'bg-[#064E3B]/[0.06] border-[#064E3B]/30' : 'bg-[#F8E7C9]/50 border-[#064E3B]/10'
      )}
    >
      <div className="flex items-center justify-between flex-wrap gap-2">
        <h3 className="font-bold text-[15px] text-[#064E3B] flex items-center gap-2">
          <Server className="w-4 h-4" /> Parallel Processing
        </h3>
        {hwInfo && draftSlots !== hwInfo.recommended_slots && (
          <button
            type="button"
            onClick={() => slots.setDraftSlots(hwInfo.recommended_slots)}
            className="text-[12px] font-semibold text-[#064E3B] underline underline-offset-2 hover:text-[#053d2f]"
          >
            Use recommended ({hwInfo.recommended_slots})
          </button>
        )}
      </div>

      <p className="text-[13px] mt-1 text-[#064E3B]/70 leading-relaxed max-w-2xl">
        Run multiple statement extractions at once during a scan.
        {hwInfo && (
          <>
            {' '}
            Recommended: <strong>{hwInfo.recommended_slots}</strong> (based on your{' '}
            {hwInfo.ram_gb.toFixed(0)}GB RAM, {hwInfo.cpu_cores} CPU cores).
          </>
        )}
      </p>

      <div className="flex items-center gap-3 mt-3">
        <input
          type="number"
          min={1}
          max={10}
          value={draftSlots}
          onChange={(e) => slots.setDraftSlots(clampSlots(parseInt(e.target.value, 10)))}
          className={cn(
            'w-20 h-9 px-3 rounded-lg border text-[#064E3B] font-semibold text-center transition-colors',
            isDirty ? 'border-[#064E3B]/60 ring-1 ring-[#064E3B]/20' : 'border-[#064E3B]/20'
          )}
          aria-label="Number of parallel LLM instances"
        />
        <span className="text-[12px] text-[#064E3B]/60">instances (1-10)</span>
        {isDirty && !slots.isSaving && (
          <span className="text-[11px] font-semibold text-[#064E3B]/60 flex items-center gap-1">
            <span className="w-1.5 h-1.5 rounded-full bg-[#064E3B]/60" /> Unsaved changes
          </span>
        )}
        <SaveButton slots={slots} />
      </div>
    </div>
  );
}
