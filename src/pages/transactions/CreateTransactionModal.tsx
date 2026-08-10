/**
 * Manual transaction entry.
 *
 * The escape hatch for payments no automated source captured -- cash, or a bank
 * that sends no alerts.
 */
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
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from '@/components/ui/dialog';
import { DatePicker } from '@/components/ui/date-picker';
import { InstrumentPicker } from '@/components/instruments/InstrumentPicker';
import type { useCreateTransaction } from './useCreateTransaction';

type Draft = ReturnType<typeof useCreateTransaction>;
type Instruments = React.ComponentProps<typeof InstrumentPicker>['instruments'];

/**
 * Manual transaction entry.
 *
 * The escape hatch for payments no automated source captured -- cash, or a bank
 * that sends no alerts.
 */
export default function CreateTransactionModal({
  draft,
  instruments,
}: {
  draft: Draft;
  instruments: Instruments;
}) {
  const incomplete = !draft.merchant.trim() || !draft.amount || !draft.instrumentId;

  return (
    <Dialog open={draft.isOpen} onOpenChange={draft.setIsOpen}>
      <DialogContent className="sm:max-w-[425px]">
        <DialogHeader>
          <DialogTitle>New Transaction</DialogTitle>
          <DialogDescription>
            Manually record a transaction not captured automatically.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4 py-2">
          <div className="space-y-2">
            <Label htmlFor="new-txn-merchant">Merchant</Label>
            <Input
              id="new-txn-merchant"
              value={draft.merchant}
              onChange={(e) => draft.setMerchant(e.target.value)}
              placeholder="e.g. Amazon"
            />
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-2">
              <Label htmlFor="new-txn-amount">Amount (₹)</Label>
              <Input
                id="new-txn-amount"
                type="number"
                min="0"
                step="0.01"
                value={draft.amount}
                onChange={(e) => draft.setAmount(e.target.value)}
              />
            </div>
            <div className="space-y-2">
              <Label>Direction</Label>
              <Select
                value={draft.direction}
                onValueChange={(v) => draft.setDirection(v as 'debit' | 'credit')}
              >
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
          <div className="space-y-2">
            <Label htmlFor="new-txn-date">Date</Label>
            <DatePicker id="new-txn-date" value={draft.date} onChange={draft.setDate} />
          </div>
          <div className="space-y-2">
            <Label>Instrument</Label>
            <InstrumentPicker
              value={draft.instrumentId}
              onChange={draft.setInstrumentId}
              instruments={instruments}
            />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => draft.setIsOpen(false)}>
            Cancel
          </Button>
          <Button onClick={draft.submit} disabled={draft.isCreating || incomplete}>
            {draft.isCreating ? 'Creating...' : 'Create'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
