/**
 * Rendered option pieces for the instrument select.
 */
import type { ReactNode } from 'react';
import { Check } from 'lucide-react';
import { SelectItem, SelectGroup, SelectLabel } from '@/components/ui/select';
import { instrumentIcon } from './instrumentTypes';
import { getInstrumentTitle, getInstrumentSubtitle } from './instrumentLabels';
import { cn } from '@/lib/utils';

export interface SelectableInstrument {
  id: string;
  issuer_name: string;
  instrument_type: string;
  masked_identifier?: string | null;
}

/** One option in the instrument select. */
function InstrumentOption({
  inst,
  isSelected,
}: {
  inst: SelectableInstrument;
  isSelected: boolean;
}) {
  return (
    <SelectItem
      value={inst.id}
      hideCheckmark
      className={cn(
        'py-2 px-2.5 my-0.5 rounded-xl transition-all cursor-pointer select-none outline-none pr-3',
        'focus:bg-[#064E3B]/10 focus:text-[#064E3B]',
        isSelected
          ? 'bg-[#064E3B]/[0.10] border border-[#064E3B]/25 font-medium'
          : 'hover:bg-[#064E3B]/[0.05]'
      )}
    >
      <div className="flex items-center justify-between w-full gap-3">
        <div className="flex items-center gap-3 min-w-0">
          <div className="w-8 h-8 rounded-lg bg-[#064E3B]/10 flex items-center justify-center text-[#064E3B] shrink-0 shadow-2xs group-hover:scale-105 transition-transform">
            {instrumentIcon(inst.instrument_type, 16)}
          </div>
          <div className="flex flex-col text-left min-w-0">
            <span className="font-bold text-[13px] text-[#064E3B] leading-tight truncate">
              {getInstrumentTitle(inst)}
            </span>
            <span className="text-[11px] text-[#064E3B]/65 font-medium truncate font-mono">
              {getInstrumentSubtitle(inst)}
            </span>
          </div>
        </div>

        {isSelected && (
          <div className="w-5 h-5 rounded-full bg-[#064E3B] text-white flex items-center justify-center shrink-0 shadow-2xs">
            <Check className="w-3 h-3" strokeWidth={3} />
          </div>
        )}
      </div>
    </SelectItem>
  );
}

/** Options grouped by issuer. */
export function InstrumentOptionGroup({
  icon,
  label,
  items,
  selectedId,
  showDivider,
}: {
  icon: ReactNode;
  label: string;
  items: SelectableInstrument[];
  selectedId: string;
  showDivider: boolean;
}) {
  if (items.length === 0) return null;

  return (
    <SelectGroup>
      {showDivider && <div className="border-t border-[#064E3B]/10 my-1" />}
      <SelectLabel className="flex items-center gap-1.5 text-[10px] font-extrabold uppercase tracking-wider text-[#064E3B]/70 px-2 py-1">
        {icon}
        <span>{label}</span>
        <span className="ml-auto text-[9px] px-1.5 py-0.2 rounded-full bg-[#064E3B]/10">
          {items.length}
        </span>
      </SelectLabel>
      {items.map((inst) => (
        <InstrumentOption key={inst.id} inst={inst} isSelected={inst.id === selectedId} />
      ))}
    </SelectGroup>
  );
}
