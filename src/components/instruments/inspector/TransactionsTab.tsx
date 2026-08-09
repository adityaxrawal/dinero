import { useMemo } from 'react';
import { useNavigate } from 'react-router-dom';
import { Loader2, Search, X } from 'lucide-react';
import TransactionItemCard from '@/components/transactions/TransactionItemCard';
import type { useInstrumentForm } from '../useInstrumentForm';

type Form = ReturnType<typeof useInstrumentForm>;
type Transactions = Form['recentTransactions'];

function SearchBar({
  query,
  onChange,
  onViewAll,
}: {
  query: string;
  onChange: (q: string) => void;
  onViewAll: () => void;
}) {
  return (
    <div className="flex flex-col md:flex-row md:items-center justify-between gap-2.5">
      <div className="relative flex-1">
        <Search className="w-3.5 h-3.5 absolute left-3 top-1/2 -translate-y-1/2 text-[#064E3B]/50" />
        <input
          type="text"
          placeholder="Search transactions for this card..."
          value={query}
          onChange={(e) => onChange(e.target.value)}
          className="w-full h-8 pl-8 pr-7 text-[12px] bg-[#F3EBDD]/80 border border-[#064E3B]/15 rounded-xl outline-none text-[#064E3B] placeholder:text-[#064E3B]/40 focus:border-[#064E3B]/40"
        />
        {query && (
          <button
            type="button"
            onClick={() => onChange('')}
            className="absolute right-2.5 top-1/2 -translate-y-1/2 text-[#064E3B]/50 hover:text-[#064E3B]"
          >
            <X className="w-3 h-3" />
          </button>
        )}
      </div>

      <button
        type="button"
        onClick={onViewAll}
        className="text-xs font-bold text-[#064E3B] hover:underline flex items-center gap-1 cursor-pointer shrink-0"
      >
        View All Ledger ↗
      </button>
    </div>
  );
}

function useFilteredTransactions(transactions: Transactions, query: string) {
  return useMemo(() => {
    if (!query.trim()) return transactions;
    const q = query.toLowerCase().trim();
    return transactions.filter(
      (tx) =>
        tx.merchant.toLowerCase().includes(q) ||
        (tx.category && tx.category.toLowerCase().includes(q)) ||
        tx.amount.toString().includes(q)
    );
  }, [transactions, query]);
}

export default function TransactionsTab({
  form,
  instrumentId,
  searchQuery,
  onSearchChange,
}: {
  form: Form;
  instrumentId: string;
  searchQuery: string;
  onSearchChange: (q: string) => void;
}) {
  const navigate = useNavigate();
  const filtered = useFilteredTransactions(form.recentTransactions, searchQuery);

  return (
    <div className="space-y-4 animate-in fade-in-50 duration-200">
      <SearchBar
        query={searchQuery}
        onChange={onSearchChange}
        onViewAll={() => navigate(`/transactions?instrument=${instrumentId}`)}
      />

      {filtered.length === 0 ? (
        <div className="text-center py-12 bg-[#F8E7C9]/40 rounded-2xl border border-[#064E3B]/10">
          <p className="text-xs text-[#064E3B]/60 italic">
            {searchQuery
              ? 'No matching transactions found.'
              : 'No transactions found for this account.'}
          </p>
        </div>
      ) : (
        <div className="space-y-2.5">
          {filtered.map((tx) => (
            <TransactionItemCard
              key={tx.id}
              transaction={tx}
              onClick={() => navigate(`/transactions/${tx.id}`)}
            />
          ))}

          {form.hasNextPage && !searchQuery && (
            <div className="pt-2 text-center">
              <button
                type="button"
                onClick={() => form.fetchNextPage()}
                disabled={form.isFetchingNextPage}
                className="w-full py-2.5 px-4 rounded-xl bg-[#064E3B]/10 hover:bg-[#064E3B]/20 text-[#064E3B] font-bold text-xs transition-colors flex items-center justify-center gap-2 cursor-pointer disabled:opacity-50"
              >
                {form.isFetchingNextPage ? (
                  <>
                    <Loader2 className="w-3.5 h-3.5 animate-spin" />
                    <span>Loading transactions...</span>
                  </>
                ) : (
                  <span>
                    Load More Transactions ({filtered.length} of {form.totalTxCount})
                  </span>
                )}
              </button>
            </div>
          )}

          <div className="text-center text-[11px] font-mono text-[#064E3B]/60 pt-1">
            Showing {filtered.length} of {form.totalTxCount} transactions
          </div>
        </div>
      )}
    </div>
  );
}
