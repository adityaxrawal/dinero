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
    <div className="flex flex-col gap-3 px-4 py-3.5 flex-shrink-0 border-b border-emerald-ink/10 bg-champagne/60 backdrop-blur-sm">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <h1 className="text-[15px] font-bold text-emerald-ink tracking-tight">All Transactions</h1>
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
            className="flex items-center justify-center w-7 h-7 rounded-lg transition-colors hover:bg-emerald-ink/10 text-emerald-ink/70 hover:text-emerald-ink cursor-pointer"
            onClick={onExport}
            aria-label="Export CSV"
            title="Export CSV"
          >
            <Download className="w-4 h-4" aria-hidden="true" />
          </button>
          <button
            type="button"
            className="flex items-center justify-center w-7 h-7 rounded-lg transition-colors bg-emerald-ink hover:bg-emerald-ink/90 text-champagne shadow-2xs cursor-pointer"
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
          className="absolute left-3 top-1/2 -translate-y-1/2 w-3.5 h-3.5 pointer-events-none text-emerald-ink/50"
          aria-hidden="true"
        />
        <input
          ref={searchInputRef}
          type="text"
          placeholder="Search merchant, category, amount, account..."
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          onKeyDown={(e) => e.key === 'Escape' && setSearchQuery('')}
          className="w-full pl-8 pr-12 h-8 rounded-xl text-[12px] font-medium outline-none placeholder:text-emerald-ink/40 focus:ring-1 focus:ring-emerald-ink/30 focus:border-emerald-ink/40 transition-all bg-[#F3EBDD]/60 border border-emerald-ink/15 text-emerald-ink"
          aria-label="Search transactions"
        />
        {searchQuery ? (
          <button
            type="button"
            className="absolute right-2.5 top-1/2 -translate-y-1/2 text-emerald-ink/50 hover:text-emerald-ink"
            onClick={() => setSearchQuery('')}
            aria-label="Clear search"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        ) : (
          <kbd className="absolute right-2.5 top-1/2 -translate-y-1/2 text-[10px] font-mono font-medium text-emerald-ink/40 bg-emerald-ink/5 px-1.5 py-0.5 rounded border border-emerald-ink/10 pointer-events-none">
            ⌘K
          </kbd>
        )}
      </div>
    </div>
  );
}
