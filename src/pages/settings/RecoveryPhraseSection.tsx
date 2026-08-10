/**
 * Displays and restores the database recovery phrase.
 *
 * The only way back into an encrypted database if the keychain entry is lost, so
 * it is presented with the weight that implies.
 */
import { useState } from 'react';
import { KeyRound, Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import SectionHeading from '@/components/settings/SectionHeading';
import { API } from '@/lib/ipc';
import { confirmAction } from '@/lib/confirmDialog';

const WARNING =
  "Only use this if you anticipate losing both your Mac and your Keychain. Anyone who has this 24-word phrase can decrypt your financial data on any computer — it bypasses this Mac's hardware-bound protection entirely. Keep it as secure as your data itself.";

/**
 * Displays and restores the database recovery phrase.
 *
 * The only way back into an encrypted database if the keychain entry is lost.
 */
export default function RecoveryPhraseSection() {
  const [recoveryPhrase, setRecoveryPhrase] = useState<string | null>(null);
  const [isFetchingPhrase, setIsFetchingPhrase] = useState(false);

  /** Reveals the phrase, which is fetched only on explicit request. */
  const handleViewRecoveryPhrase = async () => {
    if (!(await confirmAction(WARNING, 'Secure Backup Recovery Phrase'))) return;

    setIsFetchingPhrase(true);
    try {
      const phrase = await API.auth.getRecoveryPhrase();
      setRecoveryPhrase(phrase);
    } catch (err: unknown) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      alert('Failed to retrieve recovery phrase: ' + errorMessage);
    } finally {
      setIsFetchingPhrase(false);
    }
  };

  return (
    <section>
      <SectionHeading
        icon={KeyRound}
        title="Secure Backup Recovery Phrase"
        description="Opt-in only. Exists for the rare case where you lose both your Mac and your Keychain."
      />
      {recoveryPhrase ? (
        <div className="p-5 rounded-xl bg-amber-500/10 border border-amber-500/20 mb-4">
          <p className="text-[13px] font-semibold text-amber-700 mb-3">
            Write these 24 words down. Anyone with this phrase can decrypt your data on any
            computer.
          </p>
          <p className="font-mono text-[14px] leading-relaxed text-[#064E3B] font-medium break-words select-all p-4 bg-[#F8E7C9] rounded-lg border border-[#064E3B]/20 shadow-inner">
            {recoveryPhrase}
          </p>
        </div>
      ) : (
        <Button
          variant="outline"
          className="h-9 font-semibold border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5"
          onClick={handleViewRecoveryPhrase}
          disabled={isFetchingPhrase}
        >
          {isFetchingPhrase ? <Loader2 className="w-4 h-4 mr-2 animate-spin" /> : null}
          {isFetchingPhrase ? 'Generating…' : 'View Recovery Phrase'}
        </Button>
      )}
    </section>
  );
}
