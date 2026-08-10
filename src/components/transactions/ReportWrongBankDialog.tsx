/**
 * Lets the user correct a misidentified sending bank.
 *
 * The correction becomes a sender override, so future mail from that domain is
 * attributed correctly rather than needing the same fix repeatedly.
 */
import { useState, useEffect } from 'react';
import { Building2, AlertTriangle, Loader2 } from 'lucide-react';
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
import { Button } from '@/components/ui/button';
import { Label } from '@/components/ui/label';
import { useToast } from '@/hooks/use-toast';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';
import { useQueryClient } from '@tanstack/react-query';

interface ReportWrongBankDialogProps {
  transactionId: string;
  senderEmail: string | null;
  currentBank: string | null;
}

/**
 * Corrects a misidentified sending bank.
 *
 * The correction becomes a sender override, so future mail from that domain is
 * attributed correctly rather than needing the same fix each time.
 */
export default function ReportWrongBankDialog({
  transactionId,
  senderEmail,
  currentBank,
}: ReportWrongBankDialogProps) {
  const [open, setOpen] = useState(false);
  const [banks, setBanks] = useState<string[]>([]);
  const [selected, setSelected] = useState('');
  const [isSaving, setIsSaving] = useState(false);
  const { toast } = useToast();
  const queryClient = useQueryClient();

  const domain = senderEmail?.includes('@')
    ? (senderEmail.split('@').pop() ?? '').trim().toLowerCase()
    : '';

  useEffect(() => {
    if (!open || banks.length > 0) return;
    API.senderOverrides
      .knownBankNames()
      .then(setBanks)
      .catch(() => setBanks([]));
  }, [open, banks.length]);

  if (!domain) return null;

  /** Submits the correction as a sender override. */
  const submit = async () => {
    if (!selected) return;
    setIsSaving(true);
    try {
      await API.transactions.reportWrongBank(transactionId, domain, selected);
      queryClient.invalidateQueries({ queryKey: queryKeys.transactions.detail(transactionId) });
      queryClient.invalidateQueries({ queryKey: queryKeys.senderOverrides.all() });
      toast({
        title: 'Sender corrected',
        description: `Mail from ${domain} will be filed under ${selected}.`,
      });
      setOpen(false);
      setSelected('');
    } catch (err: unknown) {
      const e = err as { message?: string };
      toast({
        title: 'Could not save that',
        description: e?.message ?? String(err),
        variant: 'destructive',
      });
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="text-[11px] text-[#064E3B]/60 hover:text-[#064E3B] underline underline-offset-2"
      >
        Wrong bank?
      </button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="sm:max-w-[460px]">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Building2 className="w-4 h-4" /> Which bank is this?
            </DialogTitle>
            <DialogDescription>
              Dinero read this message as coming from{' '}
              <strong>{currentBank ?? 'an unrecognised bank'}</strong>. Pick the bank it really
              belongs to.
            </DialogDescription>
          </DialogHeader>

          <div className="flex flex-col gap-3 py-2">
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="wrong-bank-select">Correct bank</Label>
              <Select value={selected} onValueChange={setSelected}>
                <SelectTrigger id="wrong-bank-select">
                  <SelectValue placeholder="Choose a bank…" />
                </SelectTrigger>
                <SelectContent className="max-h-[300px]">
                  {banks.map((b) => (
                    <SelectItem key={b} value={b}>
                      {b}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="p-3 rounded-xl border border-amber-300 bg-amber-50 text-xs text-amber-900 flex items-start gap-2">
              <AlertTriangle className="w-4 h-4 mt-0.5 shrink-0" />
              <span>
                All future email from <strong className="font-mono">{domain}</strong> will be
                treated as <strong>{selected || 'the bank you pick'}</strong> — not just this
                transaction. Nothing already recorded changes, and you can undo this in Settings.
              </span>
            </div>
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => setOpen(false)} disabled={isSaving}>
              Cancel
            </Button>
            <Button onClick={submit} disabled={!selected || isSaving}>
              {isSaving && <Loader2 className="w-4 h-4 mr-1.5 animate-spin" />}
              Correct the sender
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
