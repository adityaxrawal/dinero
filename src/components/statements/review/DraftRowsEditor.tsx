/**
 * Editable table of extracted rows awaiting confirmation.
 *
 * Rows are corrected here before commit, and those corrections feed the learning
 * loop so the same layout parses better next time.
 */
import { Plus, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { DatePicker } from '@/components/ui/date-picker';
import type { DraftRow } from '@/lib/ipc';

/** One editable extracted row. */
function RowEditor({
  row,
  index,
  onUpdate,
  onRemove,
}: {
  row: DraftRow;
  index: number;
  onUpdate: (field: keyof DraftRow, value: string) => void;
  onRemove: () => void;
}) {
  const n = index + 1;
  return (
    <div className="grid grid-cols-[110px_1fr_90px_70px_28px] gap-1.5 items-center">
      <DatePicker
        size="sm"
        value={row.transaction_date}
        onChange={(val) => onUpdate('transaction_date', val)}
        aria-label={`Row ${n} date`}
      />
      <Input
        value={row.merchant_raw}
        onChange={(e) => onUpdate('merchant_raw', e.target.value)}
        aria-label={`Row ${n} merchant`}
      />
      <Input
        type="number"
        value={row.amount_minor / 100}
        onChange={(e) => onUpdate('amount_minor', e.target.value)}
        aria-label={`Row ${n} amount`}
      />
      <select
        className="h-9 rounded-md border border-input bg-background px-2 text-sm"
        value={row.direction}
        onChange={(e) => onUpdate('direction', e.target.value)}
        aria-label={`Row ${n} direction`}
      >
        <option value="debit">Debit</option>
        <option value="credit">Credit</option>
      </select>
      <Button variant="ghost" size="sm" onClick={onRemove} aria-label={`Delete row ${n}`}>
        <Trash2 className="w-3.5 h-3.5" aria-hidden="true" />
      </Button>
    </div>
  );
}

/**
 * Editable table of extracted rows.
 *
 * Corrections made here feed the learning loop, so the same layout parses better
 * next time.
 */
export default function DraftRowsEditor({
  rows,
  onUpdateRow,
  onAddRow,
  onRemoveRow,
}: {
  rows: DraftRow[];
  onUpdateRow: (index: number, field: keyof DraftRow, value: string) => void;
  onAddRow: () => void;
  onRemoveRow: (index: number) => void;
}) {
  return (
    <div className="flex-1 min-h-0 flex flex-col">
      <div className="flex items-center justify-between mb-2">
        <Label>Transactions ({rows.length})</Label>
        <Button variant="outline" size="sm" onClick={onAddRow}>
          <Plus className="w-3 h-3 mr-1" aria-hidden="true" /> Add row
        </Button>
      </div>
      <div className="flex-1 overflow-y-auto space-y-2 border rounded-md p-2">
        {rows.map((row, i) => (
          <RowEditor
            key={i}
            row={row}
            index={i}
            onUpdate={(field, value) => onUpdateRow(i, field, value)}
            onRemove={() => onRemoveRow(i)}
          />
        ))}
      </div>
    </div>
  );
}
