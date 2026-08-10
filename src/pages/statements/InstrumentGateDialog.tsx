/**
 * Asks which instrument an unattributable statement belongs to.
 *
 * Raised by the backend when identification fails. Pre-filled with whatever
 * extraction did recover, so the user confirms rather than starting blank.
 */
import { AlertTriangle, CreditCard } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { useGlobalState } from '@/lib/GlobalStateContext';
import type { useInstrumentGate } from './useInstrumentGate';

type Gate = ReturnType<typeof useInstrumentGate>;

/**
 * Asks which instrument an unattributable statement belongs to.
 *
 * Pre-filled with whatever extraction recovered, so the user confirms rather
 * than starting from blank.
 */
export default function InstrumentGateDialog({ gate }: { gate: Gate }) {
  const {
    instrumentModalOpen,
    pendingInstrumentFilename,
    pendingInstrumentReason,
    closeInstrumentModal,
  } = useGlobalState();

  return (
    <Dialog
      open={instrumentModalOpen}
      onOpenChange={(open) => {
        if (!open) closeInstrumentModal();
      }}
    >
      <DialogContent
        className="sm:max-w-[425px]"
        aria-labelledby="instrument-dialog-title"
        aria-describedby="instrument-dialog-desc"
      >
        <DialogHeader>
          <DialogTitle id="instrument-dialog-title" className="flex items-center gap-2">
            <CreditCard className="w-5 h-5 text-amber-700" aria-hidden="true" />
            Confirm Statement Details
          </DialogTitle>
          <DialogDescription id="instrument-dialog-desc">
            {pendingInstrumentFilename && <>{pendingInstrumentFilename}: </>}
            {pendingInstrumentReason ||
              'We could not automatically identify the issuer or account for this statement.'}{' '}
            Please confirm the details below so we know which account these transactions belong to.
          </DialogDescription>
        </DialogHeader>

        <div className="py-2 space-y-4">
          <div className="space-y-2">
            <Label htmlFor="instrument-issuer">Issuer / Bank Name</Label>
            <Input
              id="instrument-issuer"
              placeholder="e.g. HDFC Bank"
              value={gate.issuer}
              onChange={(e) => {
                gate.setIssuer(e.target.value);
                gate.setError(null);
              }}
              autoFocus
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="instrument-masked">Last 4 Digits (Card or Account Number)</Label>
            <Input
              id="instrument-masked"
              placeholder="e.g. 4321"
              maxLength={4}
              value={gate.masked}
              onChange={(e) => {
                gate.setMasked(e.target.value.replace(/\D/g, ''));
                gate.setError(null);
              }}
              onKeyDown={(e) => e.key === 'Enter' && gate.submit()}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="instrument-type">Account Type</Label>
            <Select value={gate.type} onValueChange={gate.setType}>
              <SelectTrigger id="instrument-type">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="credit_card">Credit Card</SelectItem>
                <SelectItem value="bank_account">Bank Account</SelectItem>
              </SelectContent>
            </Select>
          </div>
          {gate.error && (
            <p role="alert" className="text-sm text-red-700 flex items-center gap-1">
              <AlertTriangle className="w-3 h-3" aria-hidden="true" />
              {gate.error}
            </p>
          )}
        </div>

        <DialogFooter>
          <Button
            variant="outline"
            onClick={closeInstrumentModal}
            aria-label="Cancel instrument confirmation"
          >
            Cancel
          </Button>
          <Button
            onClick={gate.submit}
            disabled={!gate.issuer.trim() || !gate.masked.trim() || gate.isSubmitting}
            aria-label="Confirm statement instrument details"
          >
            {gate.isSubmitting ? 'Processing…' : 'Confirm & Continue'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
