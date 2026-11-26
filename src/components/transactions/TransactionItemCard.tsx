/**
 * One transaction row in the ledger list.
 */
import React from 'react';
import { formatMoney, toMinor } from '@/lib/formatMoney';
import { ArrowUpRight, ArrowDownRight, ChevronRight } from 'lucide-react';
import { cn } from '@/lib/utils';
import { formatCustomDate } from '@/lib/formatCustomDate';
import { formatMerchantName, getMerchantCategoryVisuals } from '@/lib/merchantFormatter';
import type { TransactionRecord } from '@/lib/ipc';

interface TransactionItemCardProps {
  transaction: TransactionRecord;
  onClick?: () => void;
  compact?: boolean;
}

/** One transaction row in the ledger list. */
export default function TransactionItemCard({
  transaction,
  onClick,
  compact = false,
}: TransactionItemCardProps) {
  const formattedMerchant = formatMerchantName(transaction.merchant);
  const visuals = getMerchantCategoryVisuals(transaction.category, transaction.merchant);

  const isExpense = transaction.amount < 0 || transaction.direction === 'debit';
  const absAmount = Math.abs(transaction.amount);

  return (
    <div
      onClick={onClick}
      className={cn(
        'group flex items-center justify-between p-3.5 rounded-2xl border transition-all cursor-pointer select-none',
        'bg-champagne/70 border-emerald-ink/10 hover:bg-emerald-ink/[0.05] hover:border-emerald-ink/25 hover:shadow-xs',
        compact && 'p-2.5 rounded-xl'
      )}
    >
      <div className="flex items-center gap-3 min-w-0 pr-2">
        <div
          className={cn(
            'w-10 h-10 rounded-xl flex items-center justify-center shrink-0 border transition-transform group-hover:scale-105 shadow-2xs',
            visuals.bgClass,
            visuals.textClass,
            compact && 'w-8 h-8 rounded-lg'
          )}
        >
          {visuals.icon}
        </div>

        <div className="flex flex-col min-w-0">
          <div className="flex items-center gap-2">
            <h4
              className={cn(
                'text-[14px] font-bold text-emerald-ink truncate tracking-tight',
                compact && 'text-[13px]'
              )}
            >
              {formattedMerchant}
            </h4>
          </div>

          <div className="flex items-center gap-2 mt-0.5">
            <span className="text-[11px] font-medium text-emerald-ink/65">
              {formatCustomDate(transaction.date)}
            </span>

            {transaction.category ? (
              <span
                className={cn(
                  'text-[11px] font-extrabold px-2 py-0.2 rounded-full uppercase tracking-wider border',
                  visuals.bgClass,
                  visuals.textClass
                )}
              >
                {transaction.category.replace('_', ' ')}
              </span>
            ) : (
              <span className="text-[11px] font-bold px-2 py-0.2 rounded-full uppercase tracking-wider bg-amber-500/15 text-amber-900 border border-amber-500/20">
                Uncategorized
              </span>
            )}
          </div>
        </div>
      </div>

      <div className="flex items-center gap-2 shrink-0">
        <div className="text-right">
          <div className="flex items-center justify-end gap-1">
            {isExpense ? (
              <ArrowUpRight className="w-3.5 h-3.5 text-red-600 shrink-0" />
            ) : (
              <ArrowDownRight className="w-3.5 h-3.5 text-emerald-600 shrink-0" />
            )}
            <span
              className={cn(
                'text-[14px] font-black font-mono tracking-tight',
                isExpense ? 'text-red-700' : 'text-emerald-700',
                compact && 'text-[13px]'
              )}
            >
              {formatMoney(toMinor(absAmount), transaction.currency, isExpense ? "debit" : "credit")}
            </span>
          </div>

          <span className="text-[11px] font-bold uppercase tracking-wider text-emerald-ink/50 block mt-0.5">
            {isExpense ? 'Expense' : 'Credit / Refund'}
          </span>
        </div>

        <ChevronRight className="w-4 h-4 text-emerald-ink/40 group-hover:text-emerald-ink group-hover:translate-x-0.5 transition-all" />
      </div>
    </div>
  );
}
