/**
 * Editable form of the values extraction managed to recover.
 */
import { FileText } from 'lucide-react';
import { cn } from '@/lib/utils';
import { Input } from '@/components/ui/input';
import { DatePicker } from '@/components/ui/date-picker';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { InstrumentPicker } from '@/components/instruments/InstrumentPicker';
import type { useUnassignedForm } from './useUnassignedForm';

const INPUT_CLASS = 'h-8 bg-white/50 focus-visible:bg-white text-[13px]';

/** One editable field. */
function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <>
      <div className="px-4 py-3 bg-emerald-ink/5 font-medium text-emerald-ink/85 flex items-center">
        {label}
      </div>
      <div className="px-4 py-2 flex items-center">{children}</div>
    </>
  );
}

/** Two fields side by side. */
function TwoColRow({
  left,
  right,
  last,
}: {
  left: React.ReactNode;
  right: React.ReactNode;
  last?: boolean;
}) {
  return (
    <div className={cn('grid grid-cols-1 md:grid-cols-2', !last && 'border-b border-emerald-ink/5')}>
      <div className="grid grid-cols-[110px_1fr] border-b md:border-b-0 md:border-r border-emerald-ink/5">
        {left}
      </div>
      <div className="grid grid-cols-[110px_1fr]">{right}</div>
    </div>
  );
}

type Form = ReturnType<typeof useUnassignedForm>;
type Instruments = React.ComponentProps<typeof InstrumentPicker>['instruments'];

/** Editable form of the values extraction recovered. */
export default function ExtractedDataForm({
  form,
  instruments,
  sourceMessageId,
}: {
  form: Form;
  instruments: Instruments;
  sourceMessageId: string | null;
}) {
  const { merchant, amount, direction, date, time, instrumentId, referenceId } = form.fields;
  const { setMerchant, setAmount, setDirection, setDate, setTime, setInstrumentId, setReferenceId } =
    form.setters;

  return (
    <div>
      <h3 className="text-sm font-semibold mb-3 text-emerald-ink flex items-center gap-2">
        <FileText className="w-4 h-4" /> Extracted Data
      </h3>
      <div className="bg-white rounded-xl border border-emerald-ink/10 overflow-hidden text-[13px]">
        <div className="grid grid-cols-[110px_1fr] border-b border-emerald-ink/5">
          <Field label="Merchant">
            <Input
              id="save-txn-merchant"
              value={merchant}
              onChange={(e) => setMerchant(e.target.value)}
              className={INPUT_CLASS}
              placeholder="Enter merchant name"
            />
          </Field>
        </div>

        <TwoColRow
          left={
            <Field label="Amount">
              <Input
                id="save-txn-amount"
                type="number"
                min="0"
                step="0.01"
                value={amount}
                onChange={(e) => setAmount(e.target.value)}
                className={cn(INPUT_CLASS, 'font-mono')}
                placeholder="0.00"
              />
            </Field>
          }
          right={
            <Field label="Direction">
              <Select
                value={direction}
                onValueChange={(v) => setDirection(v as 'debit' | 'credit')}
              >
                <SelectTrigger
                  aria-label="Direction"
                  aria-required="true"
                  className="h-8 bg-white/50 focus:bg-white text-[13px]"
                >
                  <SelectValue placeholder="Choose debit or credit" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="debit">Debit (spend)</SelectItem>
                  <SelectItem value="credit">Credit (income)</SelectItem>
                </SelectContent>
              </Select>
            </Field>
          }
        />

        <TwoColRow
          left={
            <Field label="Date & time">
              <div className="flex gap-2">
                <DatePicker id="save-txn-date" size="sm" value={date} onChange={setDate} />
                <Input
                  id="save-txn-time"
                  type="time"
                  value={time}
                  onChange={(e) => setTime(e.target.value)}
                  className={`${INPUT_CLASS} w-28`}
                  aria-label="Time (optional)"
                  title="Optional. Leave blank if the email gives no time."
                />
              </div>
            </Field>
          }
          right={
            <Field label="Instrument">
              <InstrumentPicker
                value={instrumentId}
                onChange={setInstrumentId}
                instruments={instruments}
                triggerClassName="h-8 bg-white/50 focus:bg-white text-[13px]"
              />
            </Field>
          }
        />

        <TwoColRow
          last
          left={
            <Field label="Ref ID">
              <Input
                id="save-txn-ref"
                value={referenceId}
                onChange={(e) => setReferenceId(e.target.value)}
                className={INPUT_CLASS}
                placeholder="Optional reference"
              />
            </Field>
          }
          right={
            <>
              <div className="px-4 py-3 bg-emerald-ink/5 font-medium text-emerald-ink/85 flex items-center">
                Source ID
              </div>
              <div
                className="px-4 py-3 text-emerald-ink/85 font-mono text-[11px] truncate flex items-center"
                title={sourceMessageId ?? undefined}
              >
                {sourceMessageId ?? 'N/A'}
              </div>
            </>
          }
        />
      </div>
    </div>
  );
}
