/**
 * Restores a paid licence after a reinstall lost the local licence token,
 * using the account email and the payment id from the purchase receipt.
 */
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { API } from '@/lib/ipc';
import { errorMessage } from '@/lib/utils';

const INPUT_CLASS =
  'w-full px-3 py-2 rounded-lg border text-[13px] font-medium bg-champagne/50 border-emerald-ink/20 text-emerald-ink focus:border-emerald-ink focus:ring-1 focus:ring-emerald-ink placeholder:text-emerald-ink/40';

/** Restore form, shown behind a toggle while the licence is not active. */
export default function RestoreLicenseForm({
  reload,
  setError,
}: {
  reload: () => Promise<void>;
  setError: (message: string | null) => void;
}) {
  const [open, setOpen] = useState(false);
  const [email, setEmail] = useState('');
  const [paymentId, setPaymentId] = useState('');
  const [busy, setBusy] = useState(false);

  const restore = async () => {
    setBusy(true);
    setError(null);
    try {
      await API.licensing.restore(email.trim(), paymentId.trim());
      setOpen(false);
      setEmail('');
      setPaymentId('');
      await reload();
    } catch (err: unknown) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  if (!open) {
    return (
      <Button
        variant="outline"
        className="h-9 font-semibold border-emerald-ink/20 text-emerald-ink/70 hover:bg-emerald-ink/5"
        onClick={() => setOpen(true)}
      >
        Already paid? Restore licence
      </Button>
    );
  }

  return (
    <div className="mt-6 pt-6 border-t border-emerald-ink/10 space-y-4">
      <p className="text-[13px] font-medium text-emerald-ink/70">
        Reinstalled Dinero or reset this Mac? Enter the email you bought with and the payment ID
        from your Razorpay receipt (it starts with <code>pay_</code>).
      </p>
      <div className="grid gap-4 max-w-sm">
        <input
          className={INPUT_CLASS}
          placeholder="Email"
          aria-label="Restore email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
        />
        <input
          className={INPUT_CLASS}
          placeholder="Payment ID (pay_…)"
          aria-label="Restore payment ID"
          value={paymentId}
          onChange={(e) => setPaymentId(e.target.value)}
        />
        <div className="flex gap-3">
          <Button
            className="h-9 font-semibold bg-emerald-ink text-champagne hover:bg-emerald-ink/90"
            onClick={restore}
            disabled={busy || !email.trim() || !paymentId.trim()}
          >
            {busy ? 'Restoring…' : 'Restore licence'}
          </Button>
          <Button variant="outline" className="h-9" onClick={() => setOpen(false)} disabled={busy}>
            Cancel
          </Button>
        </div>
      </div>
    </div>
  );
}
