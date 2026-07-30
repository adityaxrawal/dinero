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
 * "This isn't the right bank" — the only correction in the app whose blast
 * radius is larger than the row it is made from.
 *
 * The mistake being corrected is that Gate 1 resolved the *sender domain* to
 * the wrong bank, so the fix is necessarily domain-scoped: correcting only this
 * transaction would leave every future email from that domain making the same
 * mistake. That means one tap here changes how an unbounded number of future
 * messages are filed, which is why the scope sentence below is not optional
 * copy — the user has to see the radius before accepting it.
 *
 * The bank list is closed (the sender registry's own names) rather than free
 * text: a typo would file a whole domain under a bank name nothing else in the
 * app recognises.
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

  // A manually created transaction has no sender to relabel, so there is
  // nothing this control could do.
  if (!domain) return null;

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
      {/* This codebase's Dialog primitive exports no Trigger, so the opener is
          a plain button driving controlled state -- same shape
          PasswordPromptModal uses. */}
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

            {/* Mandatory: the user must see the blast radius before confirming. */}
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
