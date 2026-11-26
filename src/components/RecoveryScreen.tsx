/**
 * Shown instead of the app when the database cannot be opened with the stored
 * key (Keychain reset, new Mac). The user either restores the key from their
 * recovery phrase or, without one, resets app data.
 */
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import { API } from '@/lib/ipc';

/** Recovery screen for a database key mismatch. */
export default function RecoveryScreen() {
  const [phrase, setPhrase] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const restore = async () => {
    setBusy(true);
    setError(null);
    try {
      await API.recovery.restoreFromPhrase(phrase.trim());
      await API.recovery.restart();
    } catch (err: unknown) {
      setError(
        err instanceof Error ? err.message : String((err as { message?: string })?.message ?? err)
      );
      setBusy(false);
    }
  };

  const reset = async () => {
    const confirmed = window.confirm(
      'This permanently deletes all Dinero data on this Mac and starts fresh. Continue?'
    );
    if (!confirmed) return;
    setBusy(true);
    try {
      await API.recovery.resetAppData();
    } catch (err: unknown) {
      setError(String((err as { message?: string })?.message ?? err));
      setBusy(false);
    }
  };

  return (
    <main className="min-h-screen flex items-center justify-center bg-champagne/30 p-8">
      <div className="max-w-lg w-full space-y-5">
        <h1 className="text-xl font-semibold text-emerald-ink">Dinero can't open your data</h1>
        <p className="text-[13px] text-emerald-ink/80">
          The encryption key on this Mac no longer matches your database. This can happen after a
          Keychain reset, moving to a new Mac, or reinstalling macOS. Enter your recovery phrase to
          unlock it.
        </p>
        <Textarea
          aria-label="Recovery phrase"
          placeholder="12 or 24 words, separated by spaces"
          value={phrase}
          onChange={(e) => setPhrase(e.target.value)}
          disabled={busy}
          rows={3}
        />
        {error && (
          <p role="alert" className="text-[13px] font-semibold text-red-700">
            {error}
          </p>
        )}
        <div className="flex gap-3">
          <Button onClick={restore} disabled={busy || phrase.trim().split(/\s+/).length < 12}>
            Restore and restart
          </Button>
          <Button variant="outline" onClick={reset} disabled={busy}>
            Reset app data
          </Button>
        </div>
        <p className="text-[12px] text-emerald-ink/60">
          Without a recovery phrase your data can't be decrypted. Resetting deletes it and starts
          fresh.
        </p>
      </div>
    </main>
  );
}
