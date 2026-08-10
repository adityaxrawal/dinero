/**
 * Tab bar for the inspector's details, transactions and statements views.
 */
import { cn } from '@/lib/utils';

export type Tab = 'details' | 'transactions' | 'statements' | 'analytics';

const TABS: { id: Tab; label: string }[] = [
  { id: 'details', label: 'Details' },
  { id: 'transactions', label: 'Transactions' },
  { id: 'statements', label: 'Statements' },
  { id: 'analytics', label: 'Analytics' },
];

/** Tab bar for details, transactions and statements. */
export default function InspectorTabs({
  activeTab,
  onSelect,
  counts,
}: {
  activeTab: Tab;
  onSelect: (tab: Tab) => void;
  counts: Partial<Record<Tab, number>>;
}) {
  return (
    <div
      className="flex flex-shrink-0 px-5 pt-3 pb-2 gap-1.5 border-b border-[#064E3B]/10 overflow-x-auto bg-[#F8E7C9]/40"
      role="tablist"
    >
      {TABS.map((tab) => {
        const isActive = activeTab === tab.id;
        const count = counts[tab.id];
        return (
          <button
            key={tab.id}
            type="button"
            role="tab"
            aria-selected={isActive}
            className={cn(
              'flex items-center gap-1.5 px-3.5 py-1.5 text-[12px] font-bold rounded-full transition-all whitespace-nowrap cursor-pointer',
              isActive
                ? 'bg-[#064E3B] text-[#F8E7C9] shadow-xs'
                : 'text-[#064E3B]/70 hover:bg-[#064E3B]/10 hover:text-[#064E3B]'
            )}
            onClick={() => onSelect(tab.id)}
          >
            {tab.label}
            {count != null && count > 0 && (
              <span
                className={cn(
                  'px-1.5 py-0.2 text-[10px] rounded-full font-mono font-bold',
                  isActive ? 'bg-[#F8E7C9]/20 text-[#F8E7C9]' : 'bg-[#064E3B]/10 text-[#064E3B]'
                )}
              >
                {count}
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}
