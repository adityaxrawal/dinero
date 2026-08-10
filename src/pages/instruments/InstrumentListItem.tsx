import { instrumentIcon } from '@/components/instruments/instrumentTypes';
import { cn } from '@/lib/utils';
import type { InstrumentRecord } from '@/lib/ipc';

/** One account row in the master list, with its balance preview. */
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
          ? 'bg-[#064E3B] text-[#F8E7C9] border-[#064E3B] shadow-md ring-1 ring-[#064E3B]/20'
          : 'hover:bg-[#064E3B]/[0.06] text-[#064E3B] border-transparent'
      )}
    >
      <div className="flex items-center gap-2.5 min-w-0 pr-1">
        <div
          className={cn(
            'w-8 h-8 rounded-lg flex items-center justify-center shrink-0 text-xs font-bold transition-colors',
            isSelected ? 'bg-[#F8E7C9]/20 text-[#F8E7C9]' : 'bg-[#064E3B]/10 text-[#064E3B]'
          )}
        >
          {instrumentIcon(inst.instrument_type, 14)}
        </div>
        <div className="flex flex-col min-w-0">
          <span
            className={cn(
              'text-[13px] font-bold tracking-tight truncate',
              isSelected ? 'text-white' : 'text-[#064E3B]'
            )}
          >
            {inst.issuer_name || 'Account'}
          </span>
          <span
            className={cn(
              'text-[11px] font-mono font-medium truncate opacity-70',
              isSelected ? 'text-[#F8E7C9]' : 'text-[#064E3B]/70'
            )}
          >
            ••{inst.masked_identifier}
          </span>
        </div>
      </div>

      {/* Balance Preview Pill */}
      <div className="text-right shrink-0">
        <span
          className={cn(
            'text-[12px] font-extrabold font-mono tracking-tight block',
            isSelected ? 'text-white' : isNeg ? 'text-red-700' : 'text-[#064E3B]'
          )}
        >
          ₹
          {Math.abs(bal).toLocaleString(undefined, {
            minimumFractionDigits: 2,
            maximumFractionDigits: 2,
          })}
        </span>
      </div>
    </button>
  );
}
