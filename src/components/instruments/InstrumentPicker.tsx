/**
 * Searchable instrument chooser for forms and filters.
 */
import {
  Select,
  SelectTrigger,
  SelectValue,
  SelectContent,
  SelectItem,
} from '@/components/ui/select';

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
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger aria-label="Instrument" className={triggerClassName}>
        <SelectValue placeholder="Select instrument" />
      </SelectTrigger>
      <SelectContent>
        {instruments.map((inst) => (
          <SelectItem key={inst.id} value={inst.id}>
            {inst.issuer_name} •••• {inst.masked_identifier}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
