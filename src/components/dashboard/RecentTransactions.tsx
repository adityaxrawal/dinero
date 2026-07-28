import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Activity, Plus } from 'lucide-react';
import { cn, formatRelativeDate } from '@/lib/utils';
import type { TransactionRecord } from '@/lib/ipc';

const HIGHLIGHT_DURATION_MS = 2500;

/**
 * TASK-RT-005 (Doc 30): "near-real-time," never "real-time" -- updates only
 * happen while the app is open/backgrounded (Gmail smart-polling,
 * `transaction_created` invalidation), never while asleep or fully quit, so
 * the copy must not imply instant push delivery. Kept as a plain relative
 * string ("just now" / "2 min ago") rather than reusing `formatRelativeDate`
 * (day-granularity only -- "Today"/"Yesterday" -- useless for a timestamp
 * that updates within the same session).
 */
export function formatLastSynced(syncedAt: Date, now: Date = new Date()): string {
  const seconds = Math.max(0, Math.floor((now.getTime() - syncedAt.getTime()) / 1000));
  if (seconds < 30) return 'just now';
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min${minutes === 1 ? '' : 's'} ago`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ago`;
}

interface RecentTransactionsProps {
  transactions: TransactionRecord[];
}

/**
 * TASK-RT-005: the Dashboard's "Recent Ledger" widget, extracted so the
 * new-row highlight/animation and "near-real-time" copy are independently
 * testable. Reactive purely off whatever `transactions` prop the parent's
 * `useTransactionsList` query currently holds -- `useIpcQueryInvalidation`'s
 * existing `transaction_created` -> `queryKeys.transactions.all()`
 * invalidation is what actually causes a re-fetch; this component only
 * needs to notice the resulting prop change and animate it.
 */
export default function RecentTransactions({ transactions }: RecentTransactionsProps) {
  const navigate = useNavigate();
  const seenIdsRef = useRef<Set<string> | null>(null);
  const [highlightedIds, setHighlightedIds] = useState<Set<string>>(new Set());
  const [lastSyncedAt, setLastSyncedAt] = useState<Date>(() => new Date());
  const [, forceTick] = useState(0);

  useEffect(() => {
    const currentIds = new Set(transactions.map((t) => t.id));
    const previouslySeen = seenIdsRef.current;
    seenIdsRef.current = currentIds;

    if (previouslySeen === null) {
      // First render -- nothing to diff against, nothing is "new."
      return;
    }
    const newIds = [...currentIds].filter((id) => !previouslySeen.has(id));
    if (newIds.length === 0) {
      return;
    }
    setHighlightedIds(new Set(newIds));
    setLastSyncedAt(new Date());
    const timer = setTimeout(() => setHighlightedIds(new Set()), HIGHLIGHT_DURATION_MS);
    return () => clearTimeout(timer);
  }, [transactions]);

  // Re-renders once a minute purely so the "last synced" label's relative
  // wording keeps advancing even when nothing else about the list changes.
  useEffect(() => {
    const interval = setInterval(() => forceTick((t) => t + 1), 60_000);
    return () => clearInterval(interval);
  }, []);

  return (
    <section aria-label="Recent transactions">
      <div className="flex items-center justify-between mb-1">
        <h2 className="heading-sm">Recent Ledger</h2>
        <button
          type="button"
          className="text-xs font-medium hover:underline"
          style={{ color: '#064E3B' }}
          onClick={() => navigate('/transactions')}
        >
          View all →
        </button>
      </div>
      <p
        className="text-[11px] mb-2"
        style={{ color: 'var(--text-muted)' }}
        data-testid="last-synced-label"
      >
        Near-real-time · Last synced {formatLastSynced(lastSyncedAt)}
      </p>

      <div className="card-champagne overflow-hidden">
        {transactions.length === 0 ? (
          <div
            className="flex flex-col items-center justify-center py-12 text-center"
            role="status"
          >
            <div
              className="w-10 h-10 rounded-xl flex items-center justify-center mb-3"
              style={{ background: 'rgba(6,78,59,0.07)' }}
            >
              <Activity className="w-5 h-5" style={{ color: '#6b8a7f' }} aria-hidden="true" />
            </div>
            <p className="text-sm font-medium" style={{ color: 'var(--text-secondary)' }}>
              No transactions yet
            </p>
            <p className="text-xs mt-1" style={{ color: 'var(--text-muted)' }}>
              Sync your bank or upload a statement to get started.
            </p>
            <button
              type="button"
              className="mt-4 btn btn-primary text-xs"
              onClick={() => navigate('/statements')}
            >
              <Plus className="w-3.5 h-3.5 mr-1" aria-hidden="true" />
              Upload Statement
            </button>
          </div>
        ) : (
          <table className="data-table" aria-label="Recent transactions">
            <thead>
              <tr>
                <th>Date</th>
                <th>Merchant</th>
                <th>Category</th>
                <th className="text-right">Amount</th>
                <th>Status</th>
              </tr>
            </thead>
            <tbody>
              {transactions.map((tx) => {
                const dateLabel = formatRelativeDate(tx.date);
                const isNew = highlightedIds.has(tx.id);

                return (
                  <tr
                    key={tx.id}
                    tabIndex={0}
                    role="button"
                    data-testid={`recent-tx-row-${tx.id}`}
                    data-highlighted={isNew}
                    aria-label={`${tx.merchant}, ₹${Math.abs(tx.amount).toLocaleString()}`}
                    className="transition-colors duration-700"
                    style={isNew ? { backgroundColor: 'rgba(16,185,129,0.14)' } : undefined}
                    onClick={() => navigate(`/transactions/${tx.id}`)}
                    onKeyDown={(e) =>
                      (e.key === 'Enter' || e.key === ' ') && navigate(`/transactions/${tx.id}`)
                    }
                  >
                    <td>
                      <span
                        className="text-xs font-medium"
                        style={{ color: 'var(--text-primary)' }}
                      >
                        {dateLabel}
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
                        <span
                          className="text-sm font-medium truncate"
                          style={{ color: 'var(--text-primary)' }}
                        >
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
                      {(() => {
                        const isDebit = tx.direction === 'debit' || tx.amount < 0;
                        return (
                          <span
                            className={cn(
                              'text-sm font-semibold amount',
                              isDebit ? 'amount-debit' : 'amount-credit'
                            )}
                          >
                            {isDebit ? '−' : '+'}₹
                            {Math.abs(tx.amount).toLocaleString(undefined, {
                              minimumFractionDigits: 0,
                            })}
                          </span>
                        );
                      })()}
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
              })}
            </tbody>
          </table>
        )}
      </div>
    </section>
  );
}
