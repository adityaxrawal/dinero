import { useState } from 'react';
import { AlertTriangle, HardDrive } from 'lucide-react';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
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
      console.error('Failed to reset database:', e);
      alert('Failed to reset database');
      setIsResetting(false);
    }
  };

  return (
    <div>
      <p className="text-sm text-muted" style={{ marginBottom: '16px' }}>
        Permanently delete all your data from this device: transactions, statements, instruments,
        connected Gmail accounts, and encryption keys. This action cannot be undone.
      </p>
      <button className="btn btn-danger" onClick={() => setResetModalOpen(true)}>
        <HardDrive size={18} />
        Delete My Data
      </button>

      <Dialog
        open={resetModalOpen}
        onOpenChange={(open) => {
          if (!open && !isResetting) closeResetModal();
        }}
      >
        <DialogContent
          className="sm:max-w-[480px]"
          aria-labelledby="reset-dialog-title"
          aria-describedby="reset-dialog-desc"
        >
          {resetStep === 1 ? (
            <>
              <DialogHeader>
                <DialogTitle id="reset-dialog-title" className="flex items-center gap-2 text-red-700">
                  <AlertTriangle className="w-5 h-5" aria-hidden="true" />
                  Delete My Data
                </DialogTitle>
                <DialogDescription id="reset-dialog-desc" className="text-base pt-2">
                  This permanently deletes, on this device:
                </DialogDescription>
              </DialogHeader>
              <ul className="list-disc pl-5 space-y-1 text-sm text-muted-foreground">
                <li>All transactions, statements, and instruments</li>
                <li>All local database backups (daily and pre-migration)</li>
                <li>Your connected Gmail account(s) and stored OAuth tokens</li>
                <li>Your license/device binding (deactivated on the server)</li>
                <li>Encryption keys stored in Keychain</li>
              </ul>
              <p className="text-sm font-medium text-red-700">This cannot be undone.</p>
              <DialogFooter>
                <Button variant="outline" onClick={closeResetModal} aria-label="Cancel data deletion">
                  Cancel
                </Button>
                <Button
                  variant="destructive"
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
                <DialogTitle id="reset-dialog-title" className="flex items-center gap-2 text-red-700">
                  <AlertTriangle className="w-5 h-5" aria-hidden="true" />
                  Confirm Deletion
                </DialogTitle>
                <DialogDescription id="reset-dialog-desc">
                  Type <strong>{RESET_CONFIRM_PHRASE}</strong> below to confirm. This is your last chance to cancel.
                </DialogDescription>
              </DialogHeader>
              <div className="py-2 space-y-2">
                <Label htmlFor="reset-confirm-text">Confirmation phrase</Label>
                <Input
                  id="reset-confirm-text"
                  value={resetConfirmText}
                  onChange={(e) => setResetConfirmText(e.target.value)}
                  placeholder={RESET_CONFIRM_PHRASE}
                  autoFocus
                  aria-describedby="reset-confirm-hint"
                />
                <p id="reset-confirm-hint" className="text-xs text-muted-foreground">
                  Must match exactly, including capitalization.
                </p>
              </div>
              <DialogFooter>
                <Button variant="outline" onClick={closeResetModal} disabled={isResetting} aria-label="Cancel data deletion">
                  Cancel
                </Button>
                <Button
                  variant="destructive"
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
