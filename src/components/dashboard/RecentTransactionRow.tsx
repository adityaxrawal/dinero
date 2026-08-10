/**
 * One row of the recent-ledger widget, briefly highlighted when newly arrived.
 */
import { cn, formatRelativeDate } from '@/lib/utils';
import type { TransactionRecord } from '@/lib/ipc';

/** One row of the recent-ledger widget, highlighted when newly arrived. */
export default function RecentTransactionRow({
  tx,
  isNew,
  onOpen,
}: {
  tx: TransactionRecord;
  isNew: boolean;
  onOpen: () => void;
}) {
  const isDebit = tx.direction === 'debit' || tx.amount < 0;

  return (
    <tr
      tabIndex={0}
      role="button"
      data-testid={`recent-tx-row-${tx.id}`}
      data-highlighted={isNew}
      aria-label={`${tx.merchant}, ₹${Math.abs(tx.amount).toLocaleString()}`}
      className="transition-colors duration-700"
      style={isNew ? { backgroundColor: 'rgba(16,185,129,0.14)' } : undefined}
      onClick={onOpen}
      onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && onOpen()}
    >
      <td>
        <span className="text-xs font-medium" style={{ color: 'var(--text-primary)' }}>
          {formatRelativeDate(tx.date)}
        </span>
      </td>
      <td>
        <div className="flex items-center gap-2.5">
          <div
            className="w-7 h-7 rounded-full flex items-center justify-center text-xs font-bold flex-shrink-0"
            style={{ background: 'rgba(6,78,59,0.10)', color: '#064E3B' }}
            aria-hidden="true"
          >
            {tx.merchant.charAt(0).toUpperCase()}
          </div>
          <span className="text-sm font-medium truncate" style={{ color: 'var(--text-primary)' }}>
            {tx.merchant}
          </span>
        </div>
      </td>
      <td>
        <span
          className="text-xs px-2 py-0.5 rounded-full"
          style={{ background: 'rgba(6,78,59,0.07)', color: '#3d5a50' }}
        >
          {tx.category}
        </span>
      </td>
      <td className="text-right">
        <span
          className={cn('text-sm font-semibold amount', isDebit ? 'amount-debit' : 'amount-credit')}
        >
          {isDebit ? '−' : '+'}₹
          {Math.abs(tx.amount).toLocaleString(undefined, { minimumFractionDigits: 0 })}
        </span>
      </td>
      <td>
        <span
          className="text-[11px] font-medium px-2 py-0.5 rounded-full"
          style={{
            background:
              tx.status.toLowerCase() === 'posted'
                ? 'rgba(16,185,129,0.10)'
                : 'rgba(107,138,127,0.10)',
            color: tx.status.toLowerCase() === 'posted' ? '#10b981' : '#6b8a7f',
          }}
        >
          {tx.status}
        </span>
      </td>
    </tr>
  );
}
