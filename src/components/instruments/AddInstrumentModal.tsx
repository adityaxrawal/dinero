/**
 * Dialog for registering a new payment instrument.
 *
 * Reached both from the instruments screen and from ingestion, when a statement
 * or transaction cannot be attributed to any known account and the backend asks
 * the user to identify it.
 */
import { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
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
import { useToast } from '@/hooks/use-toast';
import { API } from '@/lib/ipc';
import { getErrorToast } from '@/lib/errorMapping';
import { queryKeys } from '@/lib/queryKeys';
import { INSTRUMENT_TYPES } from './instrumentTypes';

interface AddInstrumentModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSuccess?: (newInstrumentId: string) => void;
}

const EMPTY_FORM = {
  instrumentType: '',
  issuerName: '',
  maskedIdentifier: '',
  fullIdentifier: '',
  billingCycleDay: '',
  bankIfsc: '',
};

/** Dialog for registering a new payment instrument. */
export default function AddInstrumentModal({ open, onOpenChange, onSuccess }: AddInstrumentModalProps) {
  const { toast } = useToast();
  const queryClient = useQueryClient();
  const [form, setForm] = useState(EMPTY_FORM);
  const [isSaving, setIsSaving] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  /** Closes the dialog and clears its form. */
  const close = () => {
    onOpenChange(false);
    setForm(EMPTY_FORM);
    setFormError(null);
  };

  /** Creates the instrument and reports the outcome. */
  const handleCreate = async () => {
    if (!form.issuerName || !form.instrumentType || !form.maskedIdentifier) {
      setFormError('Issuer name, type, and masked identifier are required.');
      return;
    }
    setFormError(null);
    setIsSaving(true);
    try {
      const result = await API.instruments.create(
        form.instrumentType,
        form.issuerName,
        form.maskedIdentifier,
        form.fullIdentifier || undefined,
        form.billingCycleDay ? parseInt(form.billingCycleDay, 10) : undefined,
        form.bankIfsc || undefined
      );
      toast({ title: 'Instrument added' });
      queryClient.invalidateQueries({ queryKey: queryKeys.instruments.all() });
      if (onSuccess) {
        onSuccess(result.id);
      }
      close();
    } catch (err) {
      toast({ variant: 'destructive', ...getErrorToast(err) });
    } finally {
      setIsSaving(false);
    }
  };

  const maskedIdLabel =
    form.instrumentType === 'upi_vpa' ? 'VPA (e.g. user@upi)' : 'Last 4 Digits (e.g. 1234)';

  return (
    <Dialog open={open} onOpenChange={(o) => (o ? onOpenChange(true) : close())}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Add Instrument</DialogTitle>
          <DialogDescription>Add a new credit card, bank account, or UPI VPA.</DialogDescription>
        </DialogHeader>
        <div className="grid gap-4 py-4">
          <div className="grid gap-2">
            <Label htmlFor="add-issuer">Issuer Name</Label>
            <Input
              id="add-issuer"
              value={form.issuerName}
              onChange={(e) => setForm({ ...form, issuerName: e.target.value })}
              placeholder="e.g. HDFC Bank"
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="add-type">Type</Label>
            <Select
              value={form.instrumentType}
              onValueChange={(val) => setForm({ ...form, instrumentType: val })}
            >
              <SelectTrigger id="add-type">
                <SelectValue placeholder="Select type" />
              </SelectTrigger>
              <SelectContent>
                {INSTRUMENT_TYPES.map((t) => (
                  <SelectItem key={t.value} value={t.value}>
                    {t.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="grid gap-2">
            <Label htmlFor="add-masked">{maskedIdLabel}</Label>
            <Input
              id="add-masked"
              value={form.maskedIdentifier}
              onChange={(e) => setForm({ ...form, maskedIdentifier: e.target.value })}
              placeholder={form.instrumentType === 'upi_vpa' ? 'user@upi' : '1234'}
            />
          </div>
          {formError && (
            <p role="alert" className="text-sm text-red-700">
              {formError}
            </p>
          )}
          <div className="grid gap-2">
            <Label htmlFor="fullId">
              Full Identifier (Account / Card No){' '}
              <span className="text-muted-foreground text-xs">(Optional)</span>
            </Label>
            <Input
              id="fullId"
              value={form.fullIdentifier}
              onChange={(e) => setForm({ ...form, fullIdentifier: e.target.value })}
              placeholder="1234567890123456"
            />
          </div>
          {form.instrumentType === 'credit_card' && (
            <div className="grid gap-2">
              <Label htmlFor="billingCycle">
                Billing Cycle Day <span className="text-muted-foreground text-xs">(Optional)</span>
              </Label>
              <Input
                id="billingCycle"
                type="number"
                min="1"
                max="31"
                value={form.billingCycleDay}
                onChange={(e) => setForm({ ...form, billingCycleDay: e.target.value })}
                placeholder="15"
              />
            </div>
          )}
          {form.instrumentType === 'bank_account' && (
            <div className="grid gap-2">
              <Label htmlFor="ifsc">
                IFSC Code <span className="text-muted-foreground text-xs">(Optional)</span>
              </Label>
              <Input
                id="ifsc"
                value={form.bankIfsc}
                onChange={(e) => setForm({ ...form, bankIfsc: e.target.value })}
                placeholder="HDFC0001234"
              />
            </div>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={close}>
            Cancel
          </Button>
          <Button onClick={handleCreate} disabled={isSaving}>
            {isSaving ? 'Adding...' : 'Add Instrument'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
