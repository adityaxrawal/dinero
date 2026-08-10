/**
 * Header of the transaction feed: search, filters, and export.
 */
import { Download, Plus, Search, X } from 'lucide-react';

/** Feed header: search, filters and export. */
export default function FeedHeader({
  total,
  searchQuery,
  setSearchQuery,
  searchInputRef,
  onExport,
  onNew,
}: {
  total: number;
  searchQuery: string;
  setSearchQuery: (q: string) => void;
  searchInputRef: React.RefObject<HTMLInputElement | null>;
  onExport: () => void;
  onNew: () => void;
}) {
  return (
    <div className="flex flex-col gap-3 px-4 py-3.5 flex-shrink-0 border-b border-[#064E3B]/10 bg-[#F8E7C9]/60 backdrop-blur-sm">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <h1 className="text-[15px] font-bold text-[#064E3B] tracking-tight">All Transactions</h1>
          <span
            className="text-[11px] font-bold px-2 py-0.5 rounded-full font-mono shadow-2xs"
            style={{ background: 'rgba(6,78,59,0.08)', color: '#064E3B' }}
          >
            {total.toLocaleString()}
          </span>
        </div>

        <div className="flex items-center gap-1.5">
          <button
            type="button"
            className="flex items-center justify-center w-7 h-7 rounded-lg transition-colors hover:bg-[#064E3B]/10 text-[#064E3B]/70 hover:text-[#064E3B] cursor-pointer"
            onClick={onExport}
            aria-label="Export CSV"
            title="Export CSV"
          >
            <Download className="w-4 h-4" aria-hidden="true" />
          </button>
          <button
            type="button"
            className="flex items-center justify-center w-7 h-7 rounded-lg transition-colors bg-[#064E3B] hover:bg-[#064E3B]/90 text-[#F8E7C9] shadow-2xs cursor-pointer"
            onClick={onNew}
            aria-label="New transaction"
            title="Record transaction"
          >
            <Plus className="w-4 h-4" aria-hidden="true" />
          </button>
        </div>
      </div>

      <div className="relative w-full">
        <Search
          className="absolute left-3 top-1/2 -translate-y-1/2 w-3.5 h-3.5 pointer-events-none text-[#064E3B]/50"
          aria-hidden="true"
        />
        <input
          ref={searchInputRef}
          type="text"
          placeholder="Search merchant, category, amount, account..."
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          onKeyDown={(e) => e.key === 'Escape' && setSearchQuery('')}
          className="w-full pl-8 pr-12 h-8 rounded-xl text-[12px] font-medium outline-none placeholder:text-[#064E3B]/40 focus:ring-1 focus:ring-[#064E3B]/30 focus:border-[#064E3B]/40 transition-all bg-[#F3EBDD]/60 border border-[#064E3B]/15 text-[#064E3B]"
          aria-label="Search transactions"
        />
        {searchQuery ? (
          <button
            type="button"
            className="absolute right-2.5 top-1/2 -translate-y-1/2 text-[#064E3B]/50 hover:text-[#064E3B]"
            onClick={() => setSearchQuery('')}
            aria-label="Clear search"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        ) : (
          <kbd className="absolute right-2.5 top-1/2 -translate-y-1/2 text-[10px] font-mono font-medium text-[#064E3B]/40 bg-[#064E3B]/5 px-1.5 py-0.5 rounded border border-[#064E3B]/10 pointer-events-none">
            ⌘K
          </kbd>
        )}
      </div>
    </div>
  );
}
