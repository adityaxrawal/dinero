import { FileText } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { DatePicker } from '@/components/ui/date-picker';
import { cn } from '@/lib/utils';
import { FIELD_INPUT, LabeledField, SpecCard } from './fieldStyles';
import type { InstrumentFormProps } from './formProps';

export default function MetadataCard({ fields, setField, onSave }: InstrumentFormProps) {
  return (
    <SpecCard icon={FileText} title="Statement Metadata & Rewards" hint="Extracted & Editable">
      <div className="grid grid-cols-2 gap-2 text-[12px]">
        <LabeledField
          htmlFor="insp-due-date"
          label="Latest Bill Due Date"
          labelClassName="text-[10px]"
        >
          <DatePicker
            id="insp-due-date"
            value={fields.statementDueDate}
            onChange={(v) => setField('statementDueDate', v)}
            placeholder="Select due date"
            triggerClassName="h-9 text-[12px] font-mono font-semibold bg-[#F3EBDD]/80 border-[#064E3B]/15 text-[#064E3B] focus-visible:ring-1 focus-visible:ring-[#064E3B]/30 rounded-xl w-full"
          />
        </LabeledField>

        <LabeledField
          htmlFor="insp-min-due"
          label="Minimum Amount Due (₹)"
          labelClassName="text-[10px]"
        >
          <Input
            id="insp-min-due"
            type="number"
            step="0.01"
            value={fields.minimumDue}
            onChange={(e) => setField('minimumDue', e.target.value)}
            placeholder="e.g. 1200.00"
            className={cn(FIELD_INPUT, 'text-[12px] font-mono')}
            onKeyDown={(e) => e.key === 'Enter' && onSave()}
          />
        </LabeledField>
      </div>

      <LabeledField htmlFor="insp-rewards" label="Rewards & Cashback Summary">
        <Input
          id="insp-rewards"
          value={fields.rewardsSummary}
          onChange={(e) => setField('rewardsSummary', e.target.value)}
          placeholder="e.g. 1,250 EDGE Points • 1.5% Unlimited Cashback"
          className={FIELD_INPUT}
          onKeyDown={(e) => e.key === 'Enter' && onSave()}
        />
      </LabeledField>
    </SpecCard>
  );
}
