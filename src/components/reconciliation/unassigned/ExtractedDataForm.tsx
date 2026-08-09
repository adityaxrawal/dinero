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

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <>
      <div className="px-4 py-3 bg-[#064E3B]/5 font-medium text-[#064E3B]/70 flex items-center">
        {label}
      </div>
      <div className="px-4 py-2 flex items-center">{children}</div>
    </>
  );
}

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
    <div className={cn('grid grid-cols-1 md:grid-cols-2', !last && 'border-b border-[#064E3B]/5')}>
      <div className="grid grid-cols-[110px_1fr] border-b md:border-b-0 md:border-r border-[#064E3B]/5">
        {left}
      </div>
      <div className="grid grid-cols-[110px_1fr]">{right}</div>
    </div>
  );
}

type Form = ReturnType<typeof useUnassignedForm>;
type Instruments = React.ComponentProps<typeof InstrumentPicker>['instruments'];

export default function ExtractedDataForm({
  form,
  instruments,
  sourceMessageId,
}: {
  form: Form;
  instruments: Instruments;
  sourceMessageId: string | null;
}) {
  const { merchant, amount, direction, date, instrumentId, referenceId } = form.fields;
  const { setMerchant, setAmount, setDirection, setDate, setInstrumentId, setReferenceId } =
    form.setters;

  return (
    <div>
      <h3 className="text-sm font-semibold mb-3 text-[#064E3B] flex items-center gap-2">
        <FileText className="w-4 h-4" /> Extracted Data
      </h3>
      <div className="bg-white rounded-xl border border-[#064E3B]/10 overflow-hidden text-[13px]">
        <div className="grid grid-cols-[110px_1fr] border-b border-[#064E3B]/5">
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
                  className="h-8 bg-white/50 focus:bg-white text-[13px]"
                >
                  <SelectValue />
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
            <Field label="Date">
              <DatePicker id="save-txn-date" size="sm" value={date} onChange={setDate} />
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
              <div className="px-4 py-3 bg-[#064E3B]/5 font-medium text-[#064E3B]/70 flex items-center">
                Source ID
              </div>
              <div
                className="px-4 py-3 text-[#064E3B]/70 font-mono text-[11px] truncate flex items-center"
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
