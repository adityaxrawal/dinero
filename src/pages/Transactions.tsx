import { useMemo } from 'react';
import { useInstrumentsList } from '@/hooks/queries/useInstrumentsList';
import { useCategoriesList } from '@/hooks/queries/useCategoriesList';
import TransactionInspector from '@/components/transactions/TransactionInspector';
import { useTransactionFilters } from './transactions/useTransactionFilters';
import { useTransactionsFeed } from './transactions/useTransactionsFeed';
import { useListKeyboardNav } from './transactions/useListKeyboardNav';
import { useCreateTransaction } from './transactions/useCreateTransaction';
import { downloadTransactionsCsv } from './transactions/exportCsv';
import FeedHeader from './transactions/FeedHeader';
import FilterChips from './transactions/FilterChips';
import TransactionFeedList from './transactions/TransactionFeedList';
import CreateTransactionModal from './transactions/CreateTransactionModal';

function EmptyInspector() {
  return (
    <div className="flex-1 flex flex-col items-center justify-center h-full opacity-40 gap-3">
      <div className="w-14 h-14 border-2 border-[#064E3B] rounded-2xl border-dashed flex items-center justify-center bg-[#064E3B]/5">
        <span className="text-[#064E3B] font-extrabold text-2xl">D</span>
      </div>
      <p className="text-[#064E3B] font-semibold text-sm">
        Select a transaction to inspect details &amp; edit
      </p>
      <p className="text-[#064E3B]/60 text-xs font-mono">Use ↑ / ↓ arrow keys to quickly navigate</p>
    </div>
  );
}

export default function Transactions() {
  const { data: instruments = [] } = useInstrumentsList();
  const { data: categories = [] } = useCategoriesList();

  const filterState = useTransactionFilters();
  const { filters, searchQuery, isSearching } = filterState;
  const feed = useTransactionsFeed(filters, searchQuery, isSearching);
  const [selectedTxId, setSelectedTxId] = useListKeyboardNav(feed.transactions);
  const draft = useCreateTransaction();

  const categoryNameById = useMemo(
    () => new Map(categories.map((c) => [c.id, c.name])),
    [categories]
  );
  const instrumentById = useMemo(() => new Map(instruments.map((i) => [i.id, i])), [instruments]);

  return (
    <div className="flex h-full w-full overflow-hidden select-none">
      {/* ── Column 2: Master List (Feed) ─────────────────────────────────── */}
      <div
        className="flex-shrink-0 flex flex-col h-full border-r border-[#064E3B]/15 bg-[#F8E7C9]"
        style={{ width: '340px' }}
      >
        <FeedHeader
          total={feed.total}
          searchQuery={searchQuery}
          setSearchQuery={filterState.setSearchQuery}
          searchInputRef={filterState.searchInputRef}
          onExport={() => downloadTransactionsCsv(feed.transactions)}
          onNew={() => draft.setIsOpen(true)}
        />

        <FilterChips
          instrumentId={filters.instrument_id}
          onInstrumentChange={(id) => filterState.setFilter('instrument_id', id)}
          categoryId={filters.category_id}
          onCategoryChange={(id) => filterState.setFilter('category_id', id)}
          instruments={instruments}
          categories={categories}
          activeFilterCount={filterState.activeFilterCount}
          onReset={() => filterState.setFilters({})}
        />

        <div className="flex-1 overflow-y-auto px-2 py-2 space-y-3">
          <TransactionFeedList
            feed={feed}
            isSearching={isSearching}
            searchQuery={searchQuery}
            selectedTxId={selectedTxId}
            onSelect={setSelectedTxId}
            categoryNameById={categoryNameById}
            instrumentById={instrumentById}
          />
        </div>
      </div>

      {/* ── Column 3: Inspector Panel ──────────────────────────────────── */}
      <div className="flex-1 h-full bg-[#F8E7C9] relative overflow-hidden flex flex-col">
        {selectedTxId ? (
          <TransactionInspector
            transactionId={selectedTxId}
            onClose={() => setSelectedTxId(null)}
            categories={categories}
          />
        ) : (
          <EmptyInspector />
        )}
      </div>

      <CreateTransactionModal draft={draft} instruments={instruments} />
    </div>
  );
}
