/**
 * Highlights the least confident corrections in a run.
 *
 * Surfaced deliberately: these are where a bulk rewrite is most likely to be
 * wrong, so they are the ones worth a human look.
 */
import { ArrowRight } from 'lucide-react';
import type { MerchantCleanupSample } from '@/lib/ipc';
import GuessQuality from './GuessQuality';

/**
 * A least-confident correction from a run.
 *
 * Surfaced deliberately: these are where a bulk rewrite is most likely wrong, so
 * they are the ones worth a human look.
 */
export default function WorstMatchRow({ sample }: { sample: MerchantCleanupSample }) {
  return (
    <div className="mt-4 pt-4 border-t border-[#064E3B]/10 flex items-center gap-3 flex-wrap text-[13px]">
      <span className="text-[11px] font-semibold uppercase tracking-wide text-[#064E3B]/45">
        Worst match
      </span>
      <span className="font-mono text-[#064E3B]">{sample.merchant}</span>
      <ArrowRight className="w-3.5 h-3.5 text-[#064E3B]/35" />
      <span className="text-[#064E3B]/55 italic">read from the original email</span>
      <GuessQuality confidence={sample.confidence} />
    </div>
  );
}
