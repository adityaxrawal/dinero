/**
 * Shown when there is nothing to clean up.
 */
import { Cpu } from 'lucide-react';

/** Shown when there is nothing to clean up. */
export default function CleanupEmptyState() {
  return (
    <div className="p-5 rounded-xl border border-dashed border-emerald-ink/15 bg-champagne/40 flex items-start gap-2.5">
      <Cpu className="w-4 h-4 mt-0.5 shrink-0 text-emerald-ink/80" />
      <p className="text-[13px] text-emerald-ink/85 leading-relaxed">
        Nothing to clean up. Every merchant name currently scores above the confidence threshold, so
        there is nothing worth spending inference on.
      </p>
    </div>
  );
}
