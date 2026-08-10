/**
 * Billing cycle and due-date fields in the inspector.
 */
import { ShieldCheck } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { cn } from '@/lib/utils';
import { FIELD_INPUT, LabeledField, SpecCard } from './fieldStyles';
import type { InstrumentFormProps } from './formProps';

/** Utilisation bar against the credit limit. */
function UtilizationBar({ balance, limit }: { balance: number; limit: number }) {
  const used = Math.min(100, Math.max(0, (balance / limit) * 100));
  return (
    <div className="space-y-1 pt-1">
      <div className="flex justify-between text-[10px] font-mono text-[#064E3B]/70">
        <span>Utilization</span>
        <span>{used.toFixed(1)}% Used</span>
      </div>
      <div className="h-1.5 w-full bg-[#064E3B]/10 rounded-full overflow-hidden">
        <div
          className="h-full bg-[#064E3B] transition-all rounded-full"
          style={{ width: `${used}%` }}
        />
      </div>
    </div>
  );
}

/** Billing cycle and due-date fields. */
export default function BillingCard({
  fields,
  setField,
  onSave,
  currentBalance,
}: InstrumentFormProps & { currentBalance: number | null | undefined }) {
  /** Commits the field on Enter. */
  const saveOnEnter = (e: React.KeyboardEvent) => e.key === 'Enter' && onSave();
  const limit = parseFloat(fields.creditLimit);

  return (
    <SpecCard icon={ShieldCheck} title="Billing & Limits" hint="Cycle & Limit">
      {fields.instrumentType === 'credit_card' && (
        <>
          <LabeledField htmlFor="insp-inst-billing" label="Billing Cycle Day (1-31)">
            <Input
              id="insp-inst-billing"
              type="number"
              min="1"
              max="31"
              value={fields.billingCycleDay}
              onChange={(e) => setField('billingCycleDay', e.target.value)}
              placeholder="e.g. 15"
              className={FIELD_INPUT}
              onKeyDown={saveOnEnter}
            />
            {fields.billingCycleDay && (
              <p className="text-[10px] text-[#064E3B]/70 italic pt-0.5">
                Statements generated on the {fields.billingCycleDay}th of every month.
              </p>
            )}
          </LabeledField>

          <LabeledField htmlFor="insp-inst-limit" label="Credit Limit (₹)">
            <Input
              id="insp-inst-limit"
              type="number"
              value={fields.creditLimit}
              onChange={(e) => setField('creditLimit', e.target.value)}
              placeholder="e.g. 150000"
              className={cn(FIELD_INPUT, 'font-mono')}
              onKeyDown={saveOnEnter}
            />
            {fields.creditLimit && limit > 0 && (
              <UtilizationBar balance={currentBalance ?? 0} limit={limit} />
            )}
          </LabeledField>
        </>
      )}

      {fields.instrumentType === 'bank_account' && (
        <LabeledField htmlFor="insp-inst-ifsc" label="Bank IFSC Code">
          <Input
            id="insp-inst-ifsc"
            value={fields.bankIfsc}
            onChange={(e) => setField('bankIfsc', e.target.value)}
            placeholder="e.g. HDFC0000123"
            className={cn(FIELD_INPUT, 'uppercase font-mono')}
            onKeyDown={saveOnEnter}
          />
        </LabeledField>
      )}
    </SpecCard>
  );
}
