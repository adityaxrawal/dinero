import { useState } from 'react';
import { ChevronRight } from 'lucide-react';
import type { MerchantCleanupPreview } from '@/lib/ipc';
import { cn } from '@/lib/utils';
import QueueRow from './QueueRow';

function BankBar({
  bank,
  maxCount,
}: {
  bank: MerchantCleanupPreview['by_bank'][number];
  maxCount: number;
}) {
  return (
    <div className="flex items-center gap-3">
      <span className="text-[12px] font-semibold text-[#064E3B] w-32 shrink-0 truncate">
        {bank.bank_name}
      </span>
      <div className="flex-1 h-1.5 rounded-full bg-[#064E3B]/[0.08] overflow-hidden">
        <div
          className="h-full rounded-full bg-[#064E3B]/60"
          style={{ width: `${(bank.count / maxCount) * 100}%` }}
        />
      </div>
      <span className="text-[11px] text-[#064E3B]/55 tabular-nums shrink-0 w-24 text-right">
        {bank.count}
        {bank.no_evidence > 0 && <span className="text-amber-700"> · {bank.no_evidence} skip</span>}
      </span>
    </div>
  );
}

function SampleList({ preview }: { preview: MerchantCleanupPreview }) {
  const remaining = preview.candidate_count - preview.samples.length;

  return (
    <div className="mt-3 rounded-xl border border-[#064E3B]/10 bg-white overflow-hidden">
      <div className="px-4 py-2 bg-[#064E3B]/[0.04] text-[11px] font-semibold uppercase tracking-wide text-[#064E3B]/60">
        Worst {preview.samples.length} of {preview.candidate_count}
      </div>
      <ul className="divide-y divide-[#064E3B]/[0.07]">
        {preview.samples.map((s) => (
          <QueueRow key={s.transaction_id} sample={s} />
        ))}
      </ul>
      {remaining > 0 && (
        <div className="px-4 py-2.5 text-[11px] text-[#064E3B]/55 bg-[#064E3B]/[0.02]">
          …and {remaining} more. The queue is worked out from confidence each time, so it refreshes
          itself after every run.
        </div>
      )}
    </div>
  );
}

/** What is waiting, grouped by bank. Hidden mid-run: the counts go stale. */
export default function CleanupQueue({ preview }: { preview: MerchantCleanupPreview }) {
  const [showQueue, setShowQueue] = useState(false);

  return (
    <div className="mb-5">
      <button
        type="button"
        onClick={() => setShowQueue((v) => !v)}
        className="w-full flex items-center gap-2 text-left"
      >
        <ChevronRight
          className={cn(
            'w-4 h-4 shrink-0 text-[#064E3B]/50 transition-transform',
            showQueue && 'rotate-90'
          )}
        />
        <h3 className="font-bold text-[14px] text-[#064E3B]">What is in the queue</h3>
        <span className="text-[12px] text-[#064E3B]/55">
          {preview.by_bank.length} bank{preview.by_bank.length === 1 ? '' : 's'}
        </span>
      </button>

      <div className="mt-2.5 flex flex-col gap-1.5">
        {preview.by_bank.map((b) => (
          <BankBar key={b.bank_name} bank={b} maxCount={preview.by_bank[0].count} />
        ))}
      </div>

      {showQueue && preview.samples.length > 0 && <SampleList preview={preview} />}
    </div>
  );
}
