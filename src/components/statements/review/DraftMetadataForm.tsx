/**
 * Editable statement metadata within the review dialog.
 */
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { DatePicker } from '@/components/ui/date-picker';
import type { DraftMetadataInput } from '@/lib/ipc';

type DateField = 'dueDate' | 'statementDate' | 'billingPeriodStart' | 'billingPeriodEnd';
type MoneyField = 'currentBalance' | 'minimumDue';

const DATE_FIELDS: { id: string; label: string; field: DateField }[] = [
  { id: 'rm-due', label: 'Due date', field: 'dueDate' },
  { id: 'rm-billing-date', label: 'Billing date', field: 'statementDate' },
  { id: 'rm-period-start', label: 'Billing cycle start', field: 'billingPeriodStart' },
  { id: 'rm-period-end', label: 'Billing cycle end', field: 'billingPeriodEnd' },
];

const MONEY_FIELDS: { id: string; label: string; field: MoneyField }[] = [
  { id: 'rm-balance', label: 'Current balance (₹)', field: 'currentBalance' },
  { id: 'rm-min-due', label: 'Minimum due (₹)', field: 'minimumDue' },
];

/** Editable statement metadata within the review dialog. */
export default function DraftMetadataForm({
  metadata,
  onChange,
}: {
  metadata: DraftMetadataInput;
  onChange: (next: DraftMetadataInput) => void;
}) {
  return (
    <div className="grid grid-cols-2 gap-3">
      <div>
        <Label htmlFor="rm-bank">Bank name</Label>
        <Input
          id="rm-bank"
          value={metadata.issuerName}
          onChange={(e) => onChange({ ...metadata, issuerName: e.target.value })}
        />
      </div>
      <div>
        <Label htmlFor="rm-card">Card number (last 4)</Label>
        <Input
          id="rm-card"
          value={metadata.maskedIdentifier}
          onChange={(e) => onChange({ ...metadata, maskedIdentifier: e.target.value })}
        />
      </div>

      {DATE_FIELDS.map(({ id, label, field }) => (
        <div key={id}>
          <Label htmlFor={id}>{label}</Label>
          <DatePicker
            id={id}
            value={metadata[field] ?? ''}
            onChange={(val) => onChange({ ...metadata, [field]: val || null })}
            clearable
          />
        </div>
      ))}

      {MONEY_FIELDS.map(({ id, label, field }) => (
        <div key={id}>
          <Label htmlFor={id}>{label}</Label>
          <Input
            id={id}
            type="number"
            value={metadata[field] != null ? metadata[field]! / 100 : ''}
            onChange={(e) =>
              onChange({
                ...metadata,
                [field]: e.target.value ? Math.round(parseFloat(e.target.value) * 100) : null,
              })
            }
          />
        </div>
      ))}
    </div>
  );
}
