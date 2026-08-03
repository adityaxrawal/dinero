import {
  Select,
  SelectTrigger,
  SelectValue,
  SelectContent,
  SelectItem,
} from '@/components/ui/select';

/**
 * The plain "Issuer •••• 1234" dropdown, shared by the new-transaction dialog
 * and the unassigned inspector — both had their own identical copy.
 *
 * Not to be confused with `InstrumentSelect`, the richer searchable picker
 * that groups by instrument type and renders inside an `InfoRow`. This one is
 * a bare form control for use next to a `<Label>` or in a table cell.
 */
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
