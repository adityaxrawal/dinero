/**
 * Headline amount, merchant and date for the transaction detail page.
 */
import {
  ArrowDownLeft,
  ArrowUpRight,
  ArrowLeftRight,
  Repeat,
  Pencil,
} from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { cn, channelLabel } from '@/lib/utils';
import { formatCustomDate } from '@/lib/formatCustomDate';
import type { useTransactionForm } from '@/components/transactions/useTransactionForm';

type Form = ReturnType<typeof useTransactionForm>;

/** Editable amount field. */
function AmountInput({
  amountStr,
  setAmountStr,
  isDebit,
}: {
  amountStr: string;
  setAmountStr: (v: string) => void;
  isDebit: boolean;
}) {
  const tone = isDebit ? 'text-red-700' : 'text-emerald-700';
  return (
    <div className="flex items-center justify-center gap-1 mb-2">
      <span className={cn('text-3xl font-extrabold font-mono', tone)}>{isDebit ? '−' : '+'}₹</span>
      <div className="relative flex items-center group">
        <input
          type="number"
          step="0.01"
          value={amountStr}
          onChange={(e) => setAmountStr(e.target.value)}
          aria-label="Transaction Amount"
          className={cn(
            'bg-transparent outline-none border-b-2 border-dashed border-current focus:border-solid text-3xl font-extrabold font-mono text-center [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none pr-6',
            tone
          )}
          style={{ width: `${Math.max(amountStr.length * 18 + 28, 90)}px` }}
        />
        <Pencil
          className={cn(
            'w-3.5 h-3.5 opacity-70 group-hover:opacity-100 transition-opacity absolute right-0 pointer-events-none',
            tone
          )}
        />
      </div>
    </div>
  );
}

/** Switches between debit and credit. */
function DirectionToggle({ isDebit, onToggle }: { isDebit: boolean; onToggle: () => void }) {
  return (
    <button
      type="button"
      onClick={onToggle}
      className={cn(
        'inline-flex items-center gap-1 px-3 py-1 rounded-full text-xs font-semibold uppercase transition-opacity cursor-pointer hover:opacity-80 border',
        isDebit
          ? 'text-red-700 bg-red-500/10 border-red-500/30'
          : 'text-emerald-700 bg-emerald-500/10 border-emerald-500/30'
      )}
      title="Click to toggle Debit / Credit"
    >
      {isDebit ? (
        <ArrowUpRight className="w-3.5 h-3.5" />
      ) : (
        <ArrowDownLeft className="w-3.5 h-3.5" />
      )}
      {isDebit ? 'Debit' : 'Credit'}
    </button>
  );
}

/** Headline amount, merchant and date. */
export default function TransactionHero({
  tx,
  category,
  isDebit,
  amountStr,
  setAmountStr,
  setDirection,
}: {
  tx: NonNullable<Form['tx']>;
  category: Form['category'];
  isDebit: boolean;
  amountStr: string;
  setAmountStr: (v: string) => void;
  setDirection: Form['setDirection'];
}) {
  return (
    <div className="text-center">
      <AmountInput amountStr={amountStr} setAmountStr={setAmountStr} isDebit={isDebit} />

      <p className="text-lg font-medium">{tx.merchant_display_name}</p>
      {tx.best_event_time && (
        <p className="text-sm text-muted-foreground">{formatCustomDate(tx.best_event_time)}</p>
      )}

      <div className="flex items-center justify-center gap-2 flex-wrap mt-3">
        <DirectionToggle
          isDebit={isDebit}
          onToggle={() => setDirection(isDebit ? 'credit' : 'debit')}
        />
        {category && (
          <Badge variant="outline" className="flex items-center gap-1.5">
            <span
              className="w-2 h-2 rounded-full"
              style={{ background: category.color ?? '#064E3B' }}
              aria-hidden="true"
            />
            {category.name}
          </Badge>
        )}
        {tx.transaction_subtype && (
          <Badge variant="outline" className="flex items-center gap-1">
            <Repeat className="w-3 h-3" />
            {tx.transaction_subtype}
          </Badge>
        )}
        {tx.channel && (
          <Badge variant="outline" className="flex items-center gap-1">
            <ArrowLeftRight className="w-3 h-3" />
            {channelLabel(tx.channel)}
          </Badge>
        )}
        {tx.status && <Badge variant="outline">{tx.status}</Badge>}
      </div>
    </div>
  );
}
