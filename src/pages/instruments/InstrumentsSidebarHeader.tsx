/**
 * Header and actions for the instruments sidebar.
 */
import { Plus, Landmark, Search, X, CreditCard, Wallet, Smartphone } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { InstrumentRecord } from '@/lib/ipc';
import type { PortfolioMetrics } from './portfolioMetrics';

export type CategoryFilter = 'all' | 'credit_card' | 'bank_account' | 'upi_vpa';

const PILLS: { id: CategoryFilter; label: string; icon: React.ReactNode }[] = [
  { id: 'all', label: 'All', icon: <Wallet className="w-3 h-3" /> },
  { id: 'credit_card', label: 'Cards', icon: <CreditCard className="w-3 h-3" /> },
  { id: 'bank_account', label: 'Banks', icon: <Landmark className="w-3 h-3" /> },
  { id: 'upi_vpa', label: 'UPI', icon: <Smartphone className="w-3 h-3" /> },
];

/** Count shown in the header pill. */
const pillCount = (instruments: InstrumentRecord[], id: CategoryFilter) =>
  id === 'all' ? instruments.length : instruments.filter((i) => i.instrument_type === id).length;

/** Header and actions for the instruments sidebar. */
export default function InstrumentsSidebarHeader({
  instruments,
  metrics,
  searchQuery,
  setSearchQuery,
  selectedFilter,
  setSelectedFilter,
  onAdd,
}: {
  instruments: InstrumentRecord[];
  metrics: PortfolioMetrics;
  searchQuery: string;
  setSearchQuery: (q: string) => void;
  selectedFilter: CategoryFilter;
  setSelectedFilter: (f: CategoryFilter) => void;
  onAdd: () => void;
}) {
  return (
    <div className="flex flex-col gap-3 px-4 py-3 flex-shrink-0 border-b border-[#064E3B]/10">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <h1 className="text-[15px] font-extrabold text-[#064E3B] tracking-tight">Accounts</h1>
          <span className="text-[10px] font-extrabold px-2 py-0.5 rounded-full bg-[#064E3B]/10 text-[#064E3B]">
            {instruments.length}
          </span>
        </div>

        <button
          type="button"
          className="flex items-center gap-1 px-2.5 py-1 rounded-xl text-xs font-bold transition-all bg-[#064E3B] hover:bg-[#064E3B]/90 text-[#F8E7C9] cursor-pointer shadow-xs"
          onClick={onAdd}
          aria-label="Add account"
        >
          <Plus className="w-3.5 h-3.5" aria-hidden="true" />
          <span>Add</span>
        </button>
      </div>

      <div className="bg-[#064E3B]/[0.06] rounded-xl p-2.5 border border-[#064E3B]/10 flex justify-between items-center text-[11px] font-mono">
        <div>
          <span className="text-[#064E3B]/60 font-medium block text-[9px] uppercase tracking-wider">
            Bank Balance
          </span>
          <span className="font-bold text-[#064E3B] text-xs">
            ₹{metrics.totalBankBalance.toLocaleString()}
          </span>
        </div>
        <div className="h-6 w-[1px] bg-[#064E3B]/15" />
        <div className="text-right">
          <span className="text-[#064E3B]/60 font-medium block text-[9px] uppercase tracking-wider">
            Credit Spent
          </span>
          <span className="font-bold text-red-700 text-xs">
            ₹{metrics.totalCreditSpent.toLocaleString()}
          </span>
        </div>
      </div>

      <div className="relative">
        <Search className="w-3.5 h-3.5 absolute left-3 top-1/2 -translate-y-1/2 text-[#064E3B]/50" />
        <input
          type="text"
          placeholder="Search accounts, cards..."
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          className="w-full h-8 pl-8 pr-7 text-[12px] bg-[#F3EBDD]/80 border border-[#064E3B]/15 rounded-xl outline-none text-[#064E3B] placeholder:text-[#064E3B]/40 focus:border-[#064E3B]/40"
        />
        {searchQuery && (
          <button
            type="button"
            onClick={() => setSearchQuery('')}
            className="absolute right-2.5 top-1/2 -translate-y-1/2 text-[#064E3B]/50 hover:text-[#064E3B]"
          >
            <X className="w-3 h-3" />
          </button>
        )}
      </div>

      <div className="grid grid-cols-4 gap-1">
        {PILLS.map((pill) => {
          const count = pillCount(instruments, pill.id);
          return (
            <button
              key={pill.id}
              type="button"
              onClick={() => setSelectedFilter(pill.id)}
              className={cn(
                'flex items-center justify-center gap-1 px-1.5 py-1 text-[11px] font-bold rounded-lg transition-all cursor-pointer border truncate',
                selectedFilter === pill.id
                  ? 'bg-[#064E3B] text-[#F8E7C9] border-[#064E3B] shadow-2xs'
                  : 'bg-[#F3EBDD]/50 text-[#064E3B]/70 border-[#064E3B]/10 hover:bg-[#064E3B]/10 hover:text-[#064E3B]'
              )}
              title={`${pill.label} (${count})`}
            >
              <span className="shrink-0">{pill.icon}</span>
              <span className="truncate">{pill.label}</span>
              <span className="opacity-75 text-[9px] font-mono shrink-0 font-normal">
                ({count})
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
