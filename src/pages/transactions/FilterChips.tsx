/**
 * Active filter chips, each individually removable.
 */
import { SlidersHorizontal } from 'lucide-react';
import { cn } from '@/lib/utils';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { ALL } from './useTransactionFilters';

const CHIP =
  'h-6 text-[11px] font-semibold border-0 rounded-full px-2.5 min-w-[85px] max-w-[125px] cursor-pointer shadow-2xs';
const CHIP_ACTIVE = 'bg-[#064E3B] text-[#F8E7C9]';
const CHIP_IDLE = 'bg-[#064E3B]/5 text-[#064E3B] hover:bg-[#064E3B]/10';

/** Dropdown backing one filter chip. */
function ChipSelect({
  value,
  onChange,
  placeholder,
  allLabel,
  options,
}: {
  value: string | undefined;
  onChange: (value: string | undefined) => void;
  placeholder: string;
  allLabel: string;
  options: { id: string; label: string }[];
}) {
  return (
    <Select value={value ?? ALL} onValueChange={(val) => onChange(val === ALL ? undefined : val)}>
      <SelectTrigger className={cn(CHIP, value ? CHIP_ACTIVE : CHIP_IDLE)}>
        <SelectValue placeholder={placeholder} />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={ALL}>{allLabel}</SelectItem>
        {options.map((o) => (
          <SelectItem key={o.id} value={o.id}>
            {o.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

/** Active filter chips, each individually removable. */
export default function FilterChips({
  instrumentId,
  onInstrumentChange,
  categoryId,
  onCategoryChange,
  instruments,
  categories,
  activeFilterCount,
  onReset,
}: {
  instrumentId: string | undefined;
  onInstrumentChange: (id: string | undefined) => void;
  categoryId: string | undefined;
  onCategoryChange: (id: string | undefined) => void;
  instruments: { id: string; issuer_name: string }[];
  categories: { id: string; name: string }[];
  activeFilterCount: number;
  onReset: () => void;
}) {
  return (
    <div className="flex items-center gap-1.5 px-3.5 py-2 flex-shrink-0 border-b border-[#064E3B]/10 bg-[#064E3B]/[0.02]">
      <SlidersHorizontal
        className="w-3 h-3 flex-shrink-0 opacity-40 mx-0.5 text-[#064E3B]"
        aria-hidden="true"
      />

      <ChipSelect
        value={instrumentId}
        onChange={onInstrumentChange}
        placeholder="Accounts"
        allLabel="All Accounts"
        options={instruments.map((i) => ({ id: i.id, label: i.issuer_name }))}
      />

      <ChipSelect
        value={categoryId}
        onChange={onCategoryChange}
        placeholder="Categories"
        allLabel="All Categories"
        options={categories.map((c) => ({ id: c.id, label: c.name }))}
      />

      {activeFilterCount > 0 && (
        <button
          type="button"
          className="filter-chip text-[10px] font-bold py-0.5 px-2 rounded-full border text-red-600 border-red-500/20 hover:bg-red-500/10 cursor-pointer ml-auto"
          onClick={onReset}
          aria-label="Clear all filters"
        >
          Reset
        </button>
      )}
    </div>
  );
}
