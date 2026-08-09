import { FileWarning } from 'lucide-react';
import type { MerchantCleanupSample } from '@/lib/ipc';
import { RelativeDate } from '../SettingsPrimitives';
import GuessQuality from './GuessQuality';
import { formatAmount } from './format';

export default function QueueRow({ sample }: { sample: MerchantCleanupSample }) {
  const amount = formatAmount(sample.amount, sample.currency);
  const isCredit = sample.direction === 'credit';

  return (
    <li className="px-4 py-2.5 flex items-center justify-between gap-3">
      <div className="min-w-0">
        <div className="font-mono text-[13px] font-medium text-[#064E3B] truncate">
          {sample.merchant}
        </div>
        <div className="text-[11px] text-[#064E3B]/55 flex items-center gap-1.5 flex-wrap mt-0.5">
          {amount && (
            <span className={isCredit ? 'text-emerald-700' : undefined}>
              {isCredit ? '+' : ''}
              {amount}
            </span>
          )}
          {sample.event_time && (
            <>
              <span className="text-[#064E3B]/25">·</span>
              <RelativeDate iso={sample.event_time} />
            </>
          )}
          {!sample.has_evidence && (
            <span
              className="inline-flex items-center gap-1 text-amber-700"
              title="The original email is no longer stored, so this one will be skipped."
            >
              <FileWarning className="w-3 h-3" /> no email kept
            </span>
          )}
        </div>
      </div>
      <GuessQuality confidence={sample.confidence} />
    </li>
  );
}
