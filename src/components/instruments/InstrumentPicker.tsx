/**
 * Searchable instrument chooser for forms and filters.
 */
import { useState } from 'react';
import { Plus } from 'lucide-react';
import {
  Select,
  SelectTrigger,
  SelectValue,
  SelectContent,
  SelectItem,
} from '@/components/ui/select';
import AddInstrumentModal from './AddInstrumentModal';

/** Searchable instrument chooser. */
export function InstrumentPicker({
  value,
  onChange,
  instruments,
  triggerClassName,
}: {
  value: string;
  onChange: (id: string) => void;
  instruments: Array<{ id: string; issuer_name: string; masked_identifier?: string | null }>;
  triggerClassName?: string;
}) {
  const [isAddModalOpen, setIsAddModalOpen] = useState(false);

  const handleValueChange = (val: string) => {
    if (val === 'CREATE_NEW') {
      setIsAddModalOpen(true);
    } else {
      onChange(val);
    }
  };

  return (
    <>
      <Select value={value} onValueChange={handleValueChange}>
      <SelectTrigger aria-label="Instrument" className={triggerClassName}>
        <SelectValue placeholder="Select instrument" />
      </SelectTrigger>
      <SelectContent>
        {instruments.map((inst) => (
          <SelectItem key={inst.id} value={inst.id}>
            {inst.issuer_name} •••• {inst.masked_identifier}
          </SelectItem>
        ))}
        <div className="border-t border-[#064E3B]/10 my-1 mt-2 mx-2" />
        <SelectItem
          value="CREATE_NEW"
          hideCheckmark
          className="py-2.5 px-2.5 my-1 mx-1 rounded-xl transition-all cursor-pointer select-none outline-none focus:bg-[#064E3B]/10 hover:bg-[#064E3B]/[0.05]"
        >
          <div className="flex items-center text-[#064E3B] font-bold text-[13px]">
            <Plus className="w-4 h-4 mr-2" strokeWidth={3} />
            Create Instrument
          </div>
        </SelectItem>
      </SelectContent>
    </Select>
    {isAddModalOpen && (
      <AddInstrumentModal
        open={isAddModalOpen}
        onOpenChange={setIsAddModalOpen}
        onSuccess={onChange}
      />
    )}
    </>
  );
}
