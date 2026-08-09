import { Building } from 'lucide-react';
import { Input } from '@/components/ui/input';
import CardNumberInput from '@/components/ui/CardNumberInput';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { FIELD_SELECT, LabeledField, SpecCard } from './fieldStyles';
import type { InstrumentFormProps } from './formProps';

const NETWORKS = [
  ['Visa', 'Visa'],
  ['Mastercard', 'Mastercard'],
  ['RuPay', 'RuPay'],
  ['Amex', 'American Express'],
  ['Diners', 'Diners Club'],
] as const;

const ACCOUNT_TYPES = [
  ['Savings', 'Savings Account'],
  ['Current', 'Current Account'],
  ['Salary', 'Salary Account'],
] as const;

export default function SecurityCard({ fields, setField, onSave }: InstrumentFormProps) {
  const saveOnEnter = (e: React.KeyboardEvent) => e.key === 'Enter' && onSave();
  const isCard = fields.instrumentType === 'credit_card' || fields.instrumentType === 'debit_card';

  return (
    <SpecCard icon={Building} title="Security & Configuration" hint="Credentials">
      <LabeledField htmlFor="insp-inst-full-id" label="Full Card / Account / VPA Number">
        <CardNumberInput
          id="insp-inst-full-id"
          value={fields.fullIdentifier}
          onChange={(v) => setField('fullIdentifier', v)}
          onKeyDown={saveOnEnter}
          placeholder="4532 7603 1920 8841"
        />
      </LabeledField>

      {isCard && (
        <LabeledField label="Card Network">
          <Select value={fields.network || 'Visa'} onValueChange={(v) => setField('network', v)}>
            <SelectTrigger className={FIELD_SELECT}>
              <SelectValue placeholder="Select network" />
            </SelectTrigger>
            <SelectContent>
              {NETWORKS.map(([value, label]) => (
                <SelectItem key={value} value={value}>
                  {label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </LabeledField>
      )}

      {fields.instrumentType === 'bank_account' && (
        <LabeledField label="Account Subtype">
          <Select
            value={fields.accountType || 'Savings'}
            onValueChange={(v) => setField('accountType', v)}
          >
            <SelectTrigger className={FIELD_SELECT}>
              <SelectValue placeholder="Select account type" />
            </SelectTrigger>
            <SelectContent>
              {ACCOUNT_TYPES.map(([value, label]) => (
                <SelectItem key={value} value={value}>
                  {label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </LabeledField>
      )}

      <LabeledField htmlFor="insp-vpa" label="Associated UPI VPA">
        <Input
          id="insp-vpa"
          value={fields.upiVpa}
          onChange={(e) => setField('upiVpa', e.target.value)}
          placeholder="e.g. user@okaxis, 9876543210@upi"
          className="h-9 text-[13px] font-mono bg-[#F3EBDD]/80 border-[#064E3B]/15 text-[#064E3B] focus-visible:ring-1 focus-visible:ring-[#064E3B]/30 rounded-xl"
          onKeyDown={saveOnEnter}
        />
      </LabeledField>
    </SpecCard>
  );
}
