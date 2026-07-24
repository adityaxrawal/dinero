import { useState } from 'react';
import { useToast } from '@/hooks/use-toast';
import { getErrorToast } from '@/lib/errorMapping';
import { useInstrumentsList } from '@/hooks/queries/useInstrumentsList';
import { useResolveUnassignedTransaction } from '@/hooks/mutations/useResolveUnassignedTransaction';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import type { UnassignedTransactionRecord } from '@/lib/ipc';

/**
 * TASK-FE-013: the "Save as Transaction" resolution for an unassigned
 * item -- reuses the same field set as the Transactions page's New
 * Transaction modal, pre-filled from whatever the extraction already
 * recovered (merchant/amount/direction/date). Instrument has no reliable
 * prefill -- it's usually the actual failure point for
 * `issuer_name_not_found` -- so it's always a required dropdown.
 */
export default function SaveAsTransactionForm({
  record,
  onCancel,
  onSaved,
}: {
  record: UnassignedTransactionRecord;
  onCancel: () => void;
  onSaved: () => void;
}) {
  const { toast } = useToast();
  const { data: instruments = [] } = useInstrumentsList();
  const resolveManually = useResolveUnassignedTransaction();

  const [merchant, setMerchant] = useState(record.merchant_raw ?? '');
  const [amount, setAmount] = useState(
    record.amount_minor != null ? (record.amount_minor / 100).toString() : ''
  );
  const [direction, setDirection] = useState<'debit' | 'credit'>(
    record.direction === 'credit' ? 'credit' : 'debit'
  );
  const [date, setDate] = useState(record.event_time ? record.event_time.slice(0, 10) : '');
  const [instrumentId, setInstrumentId] = useState('');
  const [referenceId, setReferenceId] = useState('');

  const canSubmit = merchant.trim() && amount && date && instrumentId;

  const handleSubmit = () => {
    resolveManually.mutate(
      {
        id: record.id,
        amountMinor: Math.round(parseFloat(amount) * 100),
        currency: record.currency ?? 'INR',
        direction,
        eventTime: `${date} 00:00:00`,
        merchantName: merchant.trim(),
        instrumentId,
        referenceId: referenceId.trim() || undefined,
      },
      {
        onSuccess: () => {
          toast({ title: 'Transaction saved' });
          onSaved();
        },
        onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }),
      }
    );
  };

  return (
    <div className="mt-4 p-4 rounded-xl border border-[#064E3B]/10 bg-white/60 space-y-3">
      <div className="space-y-1.5">
        <Label htmlFor="save-txn-merchant">Merchant</Label>
        <Input
          id="save-txn-merchant"
          value={merchant}
          onChange={(e) => setMerchant(e.target.value)}
        />
      </div>
      <div className="grid grid-cols-2 gap-3">
        <div className="space-y-1.5">
          <Label htmlFor="save-txn-amount">Amount</Label>
          <Input
            id="save-txn-amount"
            type="number"
            min="0"
            step="0.01"
            value={amount}
            onChange={(e) => setAmount(e.target.value)}
          />
        </div>
        <div className="space-y-1.5">
          <Label>Direction</Label>
          <Select value={direction} onValueChange={(v) => setDirection(v as 'debit' | 'credit')}>
            <SelectTrigger aria-label="Direction">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="debit">Debit (spend)</SelectItem>
              <SelectItem value="credit">Credit (income)</SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>
      <div className="space-y-1.5">
        <Label htmlFor="save-txn-date">Date</Label>
        <Input
          id="save-txn-date"
          type="date"
          value={date}
          onChange={(e) => setDate(e.target.value)}
        />
      </div>
      <div className="space-y-1.5">
        <Label>Instrument</Label>
        <Select value={instrumentId} onValueChange={setInstrumentId}>
          <SelectTrigger aria-label="Instrument">
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
      </div>
      <div className="space-y-1.5">
        <Label htmlFor="save-txn-ref">Reference ID (optional)</Label>
        <Input
          id="save-txn-ref"
          value={referenceId}
          onChange={(e) => setReferenceId(e.target.value)}
        />
      </div>
      <div className="flex justify-end gap-2 pt-2">
        <Button variant="outline" onClick={onCancel}>
          Cancel
        </Button>
        <Button onClick={handleSubmit} disabled={!canSubmit || resolveManually.isPending}>
          Save
        </Button>
      </div>
    </div>
  );
}
