import { CheckCircle2, MinusCircle, Sparkles, Timer } from 'lucide-react';
import type { MerchantCleanupProgress } from '@/lib/ipc';

interface LiveStats {
  elapsed: string;
  perMin: string;
  eta: string;
}

function Counter({
  icon,
  label,
  value,
}: {
  icon: React.ReactNode;
  label: string;
  value: React.ReactNode;
}) {
  return (
    <span className="flex items-center gap-1.5">
      {icon}
      <span className="text-[#064E3B]/60">{label}</span>
      <strong className="font-bold text-[#064E3B] tabular-nums">{value}</strong>
    </span>
  );
}

/** Rate and ETA only appear mid-run: after it ends they would be stale. */
export default function CleanupCounters({
  progress,
  live,
  isRunning,
}: {
  progress: MerchantCleanupProgress;
  live: LiveStats | null;
  isRunning: boolean;
}) {
  return (
    <div className="mt-3 grid grid-cols-2 sm:grid-cols-4 gap-x-4 gap-y-2 text-[12px]">
      <Counter
        icon={<CheckCircle2 className="w-3.5 h-3.5 text-emerald-600" />}
        label="Fixed"
        value={progress.applied}
      />
      <Counter
        icon={<MinusCircle className="w-3.5 h-3.5 text-[#064E3B]/40" />}
        label="Skipped"
        value={progress.skipped}
      />
      {live && isRunning && (
        <>
          <Counter
            icon={<Sparkles className="w-3.5 h-3.5 text-[#064E3B]/40" />}
            label="Rate"
            value={`${live.perMin}/min`}
          />
          <Counter
            icon={<Timer className="w-3.5 h-3.5 text-[#064E3B]/40" />}
            label="Left"
            value={`~${live.eta}`}
          />
        </>
      )}
    </div>
  );
}
