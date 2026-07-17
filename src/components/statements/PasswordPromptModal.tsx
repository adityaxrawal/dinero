import { useCallback, useState } from 'react';
import { Lock, AlertTriangle, Clock } from 'lucide-react';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { cn } from '@/lib/utils';
import { useToast } from '@/hooks/use-toast';
import { API } from '@/lib/ipc';
import { useGlobalState } from '@/lib/GlobalStateContext';

function formatCountdown(secs: number): string {
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  return `${m}:${s.toString().padStart(2, '0')}`;
}

/**
 * TASK-FE-012 (Doc 30): triggered by the backend's `statement_password_required`
 * event — GlobalStateContext owns that event listener + the 2.5-minute
 * countdown state (a genuinely global concern: the event can fire while the
 * user isn't even on the Statements page, e.g. during a background
 * historical scan), this component is purely the extracted modal UI over
 * that existing, already-correct state. Shows a visible countdown and
 * clear wrong-password retry messaging (attempts-remaining, distinct from
 * the max-attempts-exceeded terminal state).
 */
export default function PasswordPromptModal({ onUnlocked }: { onUnlocked: () => void }) {
  const { toast } = useToast();
  const {
    passwordModalOpen,
    pendingStatementId,
    pendingInstrumentId,
    passwordTimeoutCountdown: countdown,
    closePasswordModal,
  } = useGlobalState();

  const [password, setPassword] = useState('');
  const [passwordError, setPasswordError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);

  const close = () => {
    closePasswordModal();
    setPassword('');
    setPasswordError(null);
  };

  const submitPassword = useCallback(async () => {
    if (!pendingStatementId || !password.trim()) return;
    setIsSubmitting(true);
    setPasswordError(null);
    try {
      // I9 fix (pre-existing): the backend resolves (never throws) for both
      // wrong-password and max-attempts-exceeded outcomes — `status`, not
      // promise rejection, is what distinguishes them.
      const result = await API.statements.submitPassword(pendingStatementId, pendingInstrumentId, password);

      if (result.status === 'unlocked') {
        close();
        toast({ title: 'Password Accepted', description: 'Retrying statement extraction…' });
        onUnlocked();
      } else if (result.status === 'max_attempts_exceeded') {
        close();
        toast({
          variant: 'destructive',
          title: 'Too Many Attempts',
          description: 'This statement is locked after 3 incorrect password attempts. Please re-upload it to try again.',
        });
        onUnlocked();
      } else {
        const remaining = result.attempts_remaining;
        setPasswordError(
          remaining != null ? `Incorrect password — ${remaining} attempt${remaining === 1 ? '' : 's'} remaining` : 'Incorrect password',
        );
      }
    } catch {
      setPasswordError('Incorrect password');
    } finally {
      setIsSubmitting(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingStatementId, pendingInstrumentId, password, toast, onUnlocked]);

  return (
    <Dialog
      open={passwordModalOpen}
      onOpenChange={(open) => {
        if (!open) close();
      }}
    >
      <DialogContent className="sm:max-w-[425px]" aria-labelledby="password-dialog-title" aria-describedby="password-dialog-desc">
        <DialogHeader>
          <DialogTitle id="password-dialog-title" className="flex items-center gap-2">
            <Lock className="w-5 h-5 text-red-700" aria-hidden="true" />
            Password Required
          </DialogTitle>
          <DialogDescription id="password-dialog-desc">
            The uploaded statement is encrypted. Please provide the PDF password to continue processing.
          </DialogDescription>
        </DialogHeader>

        <div
          className={cn(
            'flex items-center gap-2 text-sm px-3 py-2 rounded-md border',
            countdown <= 30 ? 'text-red-700 bg-destructive/10 border-destructive/20' : 'text-muted-foreground bg-secondary border-border',
          )}
          role="timer"
          aria-live="polite"
          aria-label={`Time remaining to enter password: ${formatCountdown(countdown)}`}
        >
          <Clock className="w-4 h-4 shrink-0" aria-hidden="true" />
          <span>Time remaining: <strong>{formatCountdown(countdown)}</strong></span>
        </div>

        <div className="py-2 space-y-2">
          <Label htmlFor="pdf-password">PDF Password</Label>
          <Input
            id="pdf-password"
            type="password"
            placeholder="Enter PDF password"
            value={password}
            onChange={(e) => {
              setPassword(e.target.value);
              setPasswordError(null);
            }}
            onKeyDown={(e) => e.key === 'Enter' && submitPassword()}
            aria-invalid={!!passwordError}
            aria-describedby={passwordError ? 'password-error' : undefined}
            autoFocus
          />
          {passwordError && (
            <p id="password-error" role="alert" className="text-sm text-red-700 flex items-center gap-1">
              <AlertTriangle className="w-3 h-3" aria-hidden="true" />
              {passwordError}
            </p>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={close} aria-label="Cancel password entry">
            Cancel
          </Button>
          <Button onClick={submitPassword} disabled={!password.trim() || isSubmitting} aria-label="Submit PDF password">
            {isSubmitting ? 'Unlocking…' : 'Unlock & Parse'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
