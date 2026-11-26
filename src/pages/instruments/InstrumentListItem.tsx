/**
 * One instrument in the list, with its balance and cycle summary.
 */
import { instrumentIcon } from '@/components/instruments/instrumentTypes';
import { cn } from '@/lib/utils';
import type { InstrumentRecord } from '@/lib/ipc';
import { formatMoney, toMinor } from '@/lib/formatMoney';

/** One instrument in the list, with balance and cycle summary. */
export default function InstrumentListItem({
  inst,
  isSelected,
  onSelect,
}: {
  inst: InstrumentRecord;
  isSelected: boolean;
  onSelect: () => void;
}) {
  const bal = inst.current_balance ?? 0;
  const isNeg = bal < 0;

  return (
    <button
      type="button"
      onClick={onSelect}
      className={cn(
        'flex items-center justify-between w-full text-left px-3 py-2.5 mx-2 rounded-xl transition-all max-w-[calc(100%-16px)] cursor-pointer select-none border',
        isSelected
          ? 'bg-emerald-ink text-champagne border-emerald-ink shadow-md ring-1 ring-emerald-ink/20'
          : 'hover:bg-emerald-ink/[0.06] text-emerald-ink border-transparent'
      )}
    >
      <div className="flex items-center gap-2.5 min-w-0 pr-1">
        <div
          className={cn(
            'w-8 h-8 rounded-lg flex items-center justify-center shrink-0 text-xs font-bold transition-colors',
            isSelected ? 'bg-champagne/20 text-champagne' : 'bg-emerald-ink/10 text-emerald-ink'
          )}
        >
          {instrumentIcon(inst.instrument_type, 14)}
        </div>
        <div className="flex flex-col min-w-0">
          <span
            className={cn(
              'text-[13px] font-bold tracking-tight truncate',
              isSelected ? 'text-white' : 'text-emerald-ink'
            )}
          >
            {inst.issuer_name || 'Account'}
          </span>
          <span
            className={cn(
              'text-[11px] font-mono font-medium truncate',
              isSelected ? 'text-champagne' : 'text-emerald-ink/85'
            )}
          >
            ••{inst.masked_identifier}
          </span>
        </div>
      </div>

      <div className="text-right shrink-0">
        <span
          className={cn(
            'text-[12px] font-extrabold font-mono tracking-tight block',
            isSelected ? 'text-white' : isNeg ? 'text-red-700' : 'text-emerald-ink'
          )}
        >
          {formatMoney(toMinor(Math.abs(bal)), 'INR')}
        </span>
      </div>
    </button>
  );
}
