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
      className="flex flex-shrink-0 px-5 pt-3 pb-2 gap-1.5 border-b border-emerald-ink/10 overflow-x-auto bg-champagne/40"
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
                ? 'bg-emerald-ink text-champagne shadow-xs'
                : 'text-emerald-ink/70 hover:bg-emerald-ink/10 hover:text-emerald-ink'
            )}
            onClick={() => onSelect(tab.id)}
          >
            {tab.label}
            {count != null && count > 0 && (
              <span
                className={cn(
                  'px-1.5 py-0.2 text-[10px] rounded-full font-mono font-bold',
                  isActive ? 'bg-champagne/20 text-champagne' : 'bg-emerald-ink/10 text-emerald-ink'
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
