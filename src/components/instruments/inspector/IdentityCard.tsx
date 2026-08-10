/**
 * Issuer, nickname and masked-identifier fields.
 *
 * The masked identifier is what ingestion matches against when attributing a
 * transaction, so editing it changes future attribution.
 */
import { CreditCard, Copy, Check } from 'lucide-react';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { cn } from '@/lib/utils';
import { FIELD_INPUT, FIELD_SELECT, LabeledField, SpecCard } from './fieldStyles';
import type { InstrumentFormProps } from './formProps';

const TYPES = [
  ['credit_card', 'Credit Card'],
  ['bank_account', 'Bank Account'],
  ['upi_vpa', 'UPI VPA'],
  ['debit_card', 'Debit Card'],
  ['wallet', 'Wallet'],
] as const;

const STATUSES = [
  ['active', 'Active'],
  ['inactive', 'Inactive'],
  ['archived', 'Archived'],
] as const;

/**
 * Issuer, nickname and masked identifier.
 *
 * The masked identifier is what ingestion matches on, so editing it changes
 * future attribution.
 */
export default function IdentityCard({
  fields,
  setField,
  onSave,
  accountId,
  copied,
  onCopyAccountId,
}: InstrumentFormProps & {
  accountId: string;
  copied: boolean;
  onCopyAccountId: () => void;
}) {
  /** Commits the field on Enter. */
  const saveOnEnter = (e: React.KeyboardEvent) => e.key === 'Enter' && onSave();

  return (
    <SpecCard icon={CreditCard} title="Identity & Specifications" hint="ID & Type">
      <LabeledField htmlFor="insp-issuer-name" label="Issuer / Institution Name">
        <Input
          id="insp-issuer-name"
          value={fields.issuerName}
          onChange={(e) => setField('issuerName', e.target.value)}
          placeholder="e.g. SBI Card, Axis Bank, HDFC Bank"
          className={FIELD_INPUT}
          onKeyDown={saveOnEnter}
        />
      </LabeledField>

      <LabeledField htmlFor="insp-nickname" label="Display Nickname">
        <Input
          id="insp-nickname"
          value={fields.nickname}
          onChange={(e) => setField('nickname', e.target.value)}
          placeholder="e.g. Primary Spender, Travel Card"
          className={FIELD_INPUT}
          onKeyDown={saveOnEnter}
        />
      </LabeledField>

      <LabeledField htmlFor="insp-masked-id" label="Masked Identifier / Tail">
        <Input
          id="insp-masked-id"
          value={fields.maskedIdentifier}
          onChange={(e) => setField('maskedIdentifier', e.target.value)}
          placeholder="e.g. 7603, user@okaxis"
          className={cn(FIELD_INPUT, 'font-mono')}
          onKeyDown={saveOnEnter}
        />
      </LabeledField>

      <LabeledField label="Instrument Type">
        <Select value={fields.instrumentType} onValueChange={(v) => setField('instrumentType', v)}>
          <SelectTrigger className={FIELD_SELECT}>
            <SelectValue placeholder="Select type" />
          </SelectTrigger>
          <SelectContent>
            {TYPES.map(([value, label]) => (
              <SelectItem key={value} value={value}>
                {label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </LabeledField>

      <LabeledField label="Status">
        <Select value={fields.status} onValueChange={(v) => setField('status', v)}>
          <SelectTrigger className={FIELD_SELECT}>
            <SelectValue placeholder="Select status" />
          </SelectTrigger>
          <SelectContent>
            {STATUSES.map(([value, label]) => (
              <SelectItem key={value} value={value}>
                {label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </LabeledField>

      <div className="flex justify-between items-center pt-2 border-t border-[#064E3B]/10">
        <span className="text-[#064E3B]/70 text-[12px] font-medium">Account ID</span>
        <div className="flex items-center gap-1">
          <span
            className="font-mono text-[11px] text-[#064E3B]/80 truncate max-w-[130px]"
            title={accountId}
          >
            {accountId}
          </span>
          <button
            type="button"
            onClick={onCopyAccountId}
            className="p-1 rounded text-[#064E3B]/60 hover:text-[#064E3B] hover:bg-[#064E3B]/10 transition-colors"
            title="Copy ID"
          >
            {copied ? (
              <Check className="w-3 h-3 text-emerald-600" />
            ) : (
              <Copy className="w-3 h-3" />
            )}
          </button>
        </div>
      </div>
    </SpecCard>
  );
}
