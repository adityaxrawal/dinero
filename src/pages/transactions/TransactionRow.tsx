/**
 * One transaction row in the feed.
 */
import { cn } from '@/lib/utils';
import { formatMoney, toMinor } from '@/lib/formatMoney';

interface RowTransaction {
  id: string;
  merchant: string;
  amount: number;
  direction: string | null;
  channel?: string | null;
  currency?: string | null;
}

/** One transaction row in the feed. */
export default function TransactionRow({
  tx,
  isSelected,
  categoryName,
  instrumentName,
  onSelect,
}: {
  tx: RowTransaction;
  isSelected: boolean;
  categoryName: string;
  instrumentName: string | undefined;
  onSelect: () => void;
}) {
  const isDebit = tx.direction === 'debit';
  // Own-funds moves are listed but kept out of totals (owner decision).
  const isInternal = tx.channel === 'internal_transfer';
  const amountTone = isSelected ? 'text-white' : isDebit ? 'text-red-700' : 'text-emerald-700';
  const badgeTone = isSelected
    ? 'bg-white/20 text-white border-white/30'
    : isDebit
      ? 'bg-red-500/10 text-red-700 border-red-500/20'
      : 'bg-emerald-500/10 text-emerald-700 border-emerald-500/20';
  const tone = isInternal && !isSelected ? 'bg-slate-500/10 text-slate-700 border-slate-500/20' : badgeTone;

  return (
    <button
      className={cn(
        'flex items-center gap-3 w-full text-left px-3 py-2.5 rounded-xl transition-all cursor-pointer border select-none',
        isSelected
          ? 'bg-emerald-ink text-champagne border-emerald-ink shadow-sm'
          : 'bg-champagne/40 hover:bg-emerald-ink/5 border-transparent text-emerald-ink'
      )}
      onClick={onSelect}
    >
      <div
        className={cn(
          'w-8 h-8 rounded-lg flex items-center justify-center text-[13px] font-bold shrink-0 transition-colors',
          isSelected ? 'bg-champagne/20 text-champagne' : 'bg-emerald-ink/10 text-emerald-ink'
        )}
      >
        {tx.merchant?.charAt(0).toUpperCase() ?? '?'}
      </div>

      <div className="flex-1 min-w-0">
        <div className="flex items-center justify-between gap-1 mb-0.5">
          <span
            className={cn(
              'font-semibold text-[13px] truncate pr-1',
              isSelected ? 'text-white' : 'text-emerald-ink'
            )}
          >
            {tx.merchant}
          </span>
          <span
            className={cn('text-[13px] font-bold font-mono whitespace-nowrap shrink-0', amountTone)}
          >
            {formatMoney(toMinor(Math.abs(tx.amount)), tx.currency ?? null, isDebit ? 'debit' : 'credit')}
          </span>
        </div>

        <div className="flex items-center justify-between text-[11px] font-medium opacity-80 gap-1 mt-0.5">
          <span className="truncate">
            {categoryName}
            {instrumentName ? ` • ${instrumentName}` : ''}
          </span>
          <span
            className={cn(
              'text-[11px] font-extrabold px-1.5 py-0.2 rounded uppercase tracking-wider border shrink-0',
              tone
            )}
          >
            {isInternal ? 'INTERNAL TRANSFER' : isDebit ? 'DEBIT' : 'CREDIT'}
          </span>
        </div>
      </div>
    </button>
  );
}
