import { useState } from 'react';
import { AlertTriangle, HardDrive } from 'lucide-react';
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
import { API } from '@/lib/ipc';

const RESET_CONFIRM_PHRASE = 'DELETE MY DATA';

/**
 * TASK-FE-014 (Doc 30): "the two-step confirmation UI (warning modal, then
 * type-to-confirm 'DELETE MY DATA') wired to auth_delete_account." Extracted
 * verbatim from the pre-existing "Reset Local Database" modal in
 * Settings.tsx (Doc 28 §4.4/§6.1/§6.3, TASK-AUTH-013) -- that modal was
 * already this exact two-step warning-then-typed-phrase flow, already wired
 * to the real backend command (`settings_delete_account`, the actual
 * Document 19 §13/§18 name; Doc 30's own task text says
 * "auth_delete_account", which doesn't exist -- this app has no
 * login/account concept, so the real command is a full local wipe, the
 * closest and only documented equivalent). This is a structural extraction
 * and rename to match Doc 30's file/section naming, not new functionality.
 */
export default function DeleteAccountSection() {
  const [resetModalOpen, setResetModalOpen] = useState(false);
  const [resetStep, setResetStep] = useState<1 | 2>(1);
  const [resetConfirmText, setResetConfirmText] = useState('');
  const [isResetting, setIsResetting] = useState(false);

  const closeResetModal = () => {
    setResetModalOpen(false);
    setResetStep(1);
    setResetConfirmText('');
  };

  const handleConfirmReset = async () => {
    if (resetConfirmText !== RESET_CONFIRM_PHRASE) return;
    setIsResetting(true);
    try {
      await API.dev.resetDatabase();
      window.location.reload();
    } catch (e) {
      if (e === 'DEV_RESTART_REQUIRED') {
        alert(
          "Development Mode: Database has been deleted successfully! However, Tauri's dev mode cannot restart automatically without breaking the Vite dev server. Please close this app and manually restart `npm run tauri dev` in your terminal."
        );
        return;
      }
      console.error('Failed to reset database:', e);
      alert('Failed to reset database');
      setIsResetting(false);
    }
  };

  return (
    <div>
      <p className="text-[13px] font-medium text-[#064E3B]/70 mb-4">
        Permanently delete all your data from this device: transactions, statements, instruments,
        connected Gmail accounts, and encryption keys. This action cannot be undone.
      </p>
      <button
        className="h-9 px-4 rounded-lg font-semibold bg-red-500/10 border border-red-500/20 text-red-600 hover:bg-red-500/20 transition-colors inline-flex items-center justify-center gap-2"
        onClick={() => setResetModalOpen(true)}
      >
        <HardDrive className="w-4 h-4" />
        Delete My Data
      </button>

      <Dialog
        open={resetModalOpen}
        onOpenChange={(open) => {
          if (!open && !isResetting) closeResetModal();
        }}
      >
        <DialogContent
          className="sm:max-w-[480px] bg-[#F8E7C9] border-[#064E3B]/20 text-[#064E3B]"
          aria-labelledby="reset-dialog-title"
          aria-describedby="reset-dialog-desc"
        >
          {resetStep === 1 ? (
            <>
              <DialogHeader>
                <DialogTitle
                  id="reset-dialog-title"
                  className="flex items-center gap-2 text-red-700"
                >
                  <AlertTriangle className="w-5 h-5" aria-hidden="true" />
                  Delete My Data
                </DialogTitle>
                <DialogDescription
                  id="reset-dialog-desc"
                  className="text-[14px] pt-2 text-[#064E3B]"
                >
                  This permanently deletes, on this device:
                </DialogDescription>
              </DialogHeader>
              <ul className="list-disc pl-5 space-y-1 text-[13px] text-[#064E3B]/70 font-medium">
                <li>All transactions, statements, and instruments</li>
                <li>All local database backups (daily and pre-migration)</li>
                <li>Your connected Gmail account(s) and stored OAuth tokens</li>
                <li>Your license/device binding (deactivated on the server)</li>
                <li>Encryption keys stored in Keychain</li>
              </ul>
              <p className="text-[13px] font-bold text-red-700">This cannot be undone.</p>
              <DialogFooter>
                <Button
                  variant="outline"
                  className="border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5"
                  onClick={closeResetModal}
                  aria-label="Cancel data deletion"
                >
                  Cancel
                </Button>
                <Button
                  className="bg-red-600 text-white hover:bg-red-700"
                  onClick={() => setResetStep(2)}
                  aria-label="I understand, continue to confirmation"
                >
                  I Understand, Continue
                </Button>
              </DialogFooter>
            </>
          ) : (
            <>
              <DialogHeader>
                <DialogTitle
                  id="reset-dialog-title"
                  className="flex items-center gap-2 text-red-700"
                >
                  <AlertTriangle className="w-5 h-5" aria-hidden="true" />
                  Confirm Deletion
                </DialogTitle>
                <DialogDescription id="reset-dialog-desc" className="text-[#064E3B]/80 font-medium">
                  Type <strong className="text-[#064E3B]">{RESET_CONFIRM_PHRASE}</strong> below to
                  confirm. This is your last chance to cancel.
                </DialogDescription>
              </DialogHeader>
              <div className="py-2 space-y-2">
                <Label htmlFor="reset-confirm-text" className="text-[#064E3B]">
                  Confirmation phrase
                </Label>
                <Input
                  id="reset-confirm-text"
                  value={resetConfirmText}
                  onChange={(e) => setResetConfirmText(e.target.value)}
                  placeholder={RESET_CONFIRM_PHRASE}
                  autoFocus
                  aria-describedby="reset-confirm-hint"
                  className="bg-[#F8E7C9]/50 border-[#064E3B]/20 focus-visible:ring-[#064E3B]"
                />
                <p id="reset-confirm-hint" className="text-xs text-[#064E3B]/60">
                  Must match exactly, including capitalization.
                </p>
              </div>
              <DialogFooter>
                <Button
                  variant="outline"
                  className="border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5"
                  onClick={closeResetModal}
                  disabled={isResetting}
                  aria-label="Cancel data deletion"
                >
                  Cancel
                </Button>
                <Button
                  className="bg-red-600 text-white hover:bg-red-700"
                  onClick={handleConfirmReset}
                  disabled={resetConfirmText !== RESET_CONFIRM_PHRASE || isResetting}
                  aria-label="Permanently delete my data"
                >
                  {isResetting ? 'Deleting…' : 'Permanently Delete'}
                </Button>
              </DialogFooter>
            </>
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}
