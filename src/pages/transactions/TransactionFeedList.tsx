/**
 * The virtualised, infinitely scrolling transaction list.
 */
import { Loader2 } from 'lucide-react';
import type { useTransactionsFeed } from './useTransactionsFeed';
import TransactionRow from './TransactionRow';

type Feed = ReturnType<typeof useTransactionsFeed>;
type Transaction = Feed['transactions'][number];

interface TransactionFeedListProps {
  feed: Feed;
  isSearching: boolean;
  searchQuery: string;
  selectedTxId: string | null;
  onSelect: (id: string) => void;
  categoryNameById: Map<string, string>;
  instrumentById: Map<string, { issuer_name: string }>;
}

/** Loads the next page as the user reaches the end. */
function LoadMore({
  sentinelRef,
  isFetching,
  onFetchNext,
}: {
  sentinelRef: React.RefObject<HTMLDivElement | null>;
  isFetching: boolean;
  onFetchNext: () => void;
}) {
  return (
    <div ref={sentinelRef} className="flex justify-center py-3">
      <button
        type="button"
        className="text-xs font-semibold px-4 py-1.5 rounded-full border border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/10 transition-colors cursor-pointer"
        onClick={onFetchNext}
        disabled={isFetching}
      >
        {isFetching ? (
          <>
            <Loader2 className="w-3.5 h-3.5 animate-spin inline mr-1.5" />
            Loading…
          </>
        ) : (
          'Load more'
        )}
      </button>
    </div>
  );
}

/** The infinitely scrolling transaction list. */
export default function TransactionFeedList({
  feed,
  isSearching,
  searchQuery,
  selectedTxId,
  onSelect,
  categoryNameById,
  instrumentById,
}: TransactionFeedListProps) {
  if (feed.loading) {
    return (
      <div className="flex flex-col items-center justify-center h-48 gap-2">
        <Loader2 className="w-5 h-5 animate-spin text-[#064E3B]/60" />
        <span className="text-xs font-medium text-[#064E3B]/60">Loading transactions…</span>
      </div>
    );
  }

  if (feed.transactions.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-48 px-4 text-center">
        <p className="text-xs font-medium text-[#064E3B]/60">
          {isSearching ? `No transactions match "${searchQuery}"` : 'No transactions found.'}
        </p>
      </div>
    );
  }

  /** Builds the props for one row, including selection state. */
  const rowProps = (tx: Transaction) => ({
    categoryName: categoryNameById.get(tx.category) || tx.category || 'Uncategorized',
    instrumentName: tx.instrument_id ? instrumentById.get(tx.instrument_id)?.issuer_name : undefined,
  });

  return (
    <div className="flex flex-col gap-3">
      {feed.grouped.map((group) => (
        <div key={group.dateLabel} className="space-y-1">
          <div className="sticky top-0 z-10 px-2 py-1 bg-[#F8E7C9]/90 backdrop-blur-xs text-[10px] font-bold text-[#064E3B]/60 uppercase tracking-wider">
            {group.dateLabel}
          </div>
          {group.items.map((tx) => (
            <TransactionRow
              key={tx.id}
              tx={tx}
              isSelected={selectedTxId === tx.id}
              onSelect={() => onSelect(tx.id)}
              {...rowProps(tx)}
            />
          ))}
        </div>
      ))}

      {!isSearching && feed.hasNextPage && (
        <LoadMore
          sentinelRef={feed.sentinelRef}
          isFetching={feed.isFetchingNextPage}
          onFetchNext={() => feed.fetchNextPage()}
        />
      )}
    </div>
  );
}
