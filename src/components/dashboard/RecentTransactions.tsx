/**
 * The dashboard's recent-ledger widget.
 *
 * Reactive purely off its props: cache invalidation driven by backend events is
 * what refetches the data, so this only has to notice the change and animate it.
 */
import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Activity, Plus } from 'lucide-react';
import type { TransactionRecord } from '@/lib/ipc';
import { formatLastSynced } from './formatLastSynced';
import RecentTransactionRow from './RecentTransactionRow';

const HIGHLIGHT_DURATION_MS = 2500;

interface RecentTransactionsProps {
  transactions: TransactionRecord[];
}

/** The dashboard's recent-ledger widget. */
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
              {transactions.map((tx) => (
                <RecentTransactionRow
                  key={tx.id}
                  tx={tx}
                  isNew={highlightedIds.has(tx.id)}
                  onOpen={() => navigate(`/transactions/${tx.id}`)}
                />
              ))}
            </tbody>
          </table>
        )}
      </div>
    </section>
  );
}
