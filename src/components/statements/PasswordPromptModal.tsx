import { useCallback, useState, useEffect } from 'react';
import { Lock, AlertTriangle, Eye, EyeOff } from 'lucide-react';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { cn } from '@/lib/utils';
import { useToast } from '@/hooks/use-toast';
import { API } from '@/lib/ipc';
import { useGlobalState } from '@/lib/GlobalStateContext';

/**
 * TASK-FE-012 (Doc 30): triggered by the backend's `statement_password_required`
 * event — GlobalStateContext owns that event listener + the 2.5-minute
 * countdown state (a genuinely global concern: the event can fire while the
 * user isn't even on the Statements page, e.g. during a background
 * historical scan), this component is purely the extracted modal UI over
 * that existing, already-correct state. Shows a visible countdown and
 * that existing, already-correct state. Shows clear wrong-password retry
 * messaging.
 */
export default function PasswordPromptModal({ onUnlocked }: { onUnlocked: () => void }) {
  const { toast } = useToast();
  const {
    passwordModalOpen,
    pendingStatementId,
    pendingInstrumentId,
    closePasswordModal,
  } = useGlobalState();

  const [password, setPassword] = useState('');
  const [passwordError, setPasswordError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [showPassword, setShowPassword] = useState(false);
  const [statementDetails, setStatementDetails] = useState<any>(null);

  useEffect(() => {
    console.log('[PasswordPromptModal] useEffect triggered. passwordModalOpen:', passwordModalOpen, 'pendingStatementId:', pendingStatementId);
    if (passwordModalOpen && pendingStatementId) {
      console.log('[PasswordPromptModal] Fetching un-processed statements to find details for statement ID:', pendingStatementId);
      API.statements.listUnprocessed().then(groups => {
        console.log('[PasswordPromptModal] Successfully fetched unprocessed statements:', groups);
        const found = groups.awaiting_password.find(s => s.statement_id === pendingStatementId);
        if (found) {
          console.log('[PasswordPromptModal] Found matching statement details:', found);
          setStatementDetails(found);
        } else {
          console.log('[PasswordPromptModal] Could not find matching statement details for ID:', pendingStatementId);
        }
      }).catch(err => {
        console.error('[PasswordPromptModal] Error fetching unprocessed statements:', err);
      });
    } else {
      console.log('[PasswordPromptModal] Clearing statement details (modal closed or no statement ID)');
      setStatementDetails(null);
    }
  }, [passwordModalOpen, pendingStatementId]);

  const close = () => {
    console.log('[PasswordPromptModal] close() called. Closing modal and resetting state.');
    closePasswordModal();
    setPassword('');
    setPasswordError(null);
  };

  const submitPassword = useCallback(async () => {
    console.log('[PasswordPromptModal] submitPassword() called.');
    console.log('[PasswordPromptModal] pendingStatementId:', pendingStatementId, 'pendingInstrumentId:', pendingInstrumentId);
    if (!pendingStatementId || !password.trim()) {
      console.log('[PasswordPromptModal] Validation failed. Missing statement ID or empty password. Aborting submission.');
      return;
    }
    console.log('[PasswordPromptModal] Validation passed. Setting isSubmitting to true.');
    setIsSubmitting(true);
    setPasswordError(null);
    try {
      console.log('[PasswordPromptModal] Making API call to API.statements.submitPassword...');
      // I9 fix (pre-existing): the backend resolves (never throws) for both
      // wrong-password and max-attempts-exceeded outcomes — `status`, not
      // promise rejection, is what distinguishes them.
      const result = await API.statements.submitPassword(pendingStatementId, pendingInstrumentId, password);
      console.log('[PasswordPromptModal] API call completed. Result:', result);

      if (result.status === 'unlocked') {
        console.log('[PasswordPromptModal] Status is "unlocked". Calling close(), showing toast, and triggering onUnlocked().');
        close();
        toast({ title: 'Password Accepted', description: 'Retrying statement extraction…' });
        onUnlocked();
      } else {
        console.log('[PasswordPromptModal] Status is NOT "unlocked" (e.g., incorrect password). Setting error state.');
        setPasswordError('Incorrect password');
      }
    } catch (error: any) {
      console.error('[PasswordPromptModal] Exception caught during API call:', error);
      const errorMessage = typeof error === 'string' ? error : error?.message || '';
      if (errorMessage.toLowerCase().includes('session has expired')) {
        setPasswordError("Session expired. Please re-upload the file.");
      } else {
        setPasswordError('Incorrect password');
      }
    } finally {
      console.log('[PasswordPromptModal] submitPassword() finally block. Setting isSubmitting to false.');
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
      <DialogContent className="sm:max-w-[1050px] w-[95vw] p-0 overflow-hidden flex flex-col max-h-[90vh] h-[750px]" aria-labelledby="password-dialog-title" aria-describedby="password-dialog-desc">
        <div className="grid grid-cols-1 md:grid-cols-[1fr_400px] flex-1 min-h-0 h-full">
          {/* Left Side: Email Context (Gmail UI) */}
          <div className="bg-white text-black border-r flex flex-col min-h-0 h-full">
            {/* Subject */}
            <div className="px-8 pt-8 pb-4">
              <h2 className="text-2xl font-normal leading-tight text-gray-900">
                {statementDetails?.subject || "Statement Context"}
              </h2>
            </div>
            
            {/* Sender Header */}
            <div className="px-8 flex items-start justify-between pb-6">
              <div className="flex items-center gap-4">
                <div className="w-12 h-12 rounded-full bg-[#1a73e8] flex items-center justify-center text-white font-medium flex-shrink-0 text-xl">
                  {statementDetails?.sender ? statementDetails.sender.charAt(0).toUpperCase() : "?"}
                </div>
                <div className="flex flex-col">
                  <div className="flex items-baseline gap-1.5 flex-wrap">
                    <span className="font-bold text-sm text-gray-900">
                      {statementDetails?.sender?.split('<')[0]?.trim() || "Unknown Sender"}
                    </span>
                    <span className="text-xs text-gray-500">
                      {statementDetails?.sender?.includes('<') ? `<${statementDetails.sender.split('<')[1]}` : ''}
                    </span>
                  </div>
                  <div className="text-xs text-gray-500 flex items-center gap-1 mt-0.5">
                    to me 
                    <svg className="w-3 h-3 text-gray-400" fill="currentColor" viewBox="0 0 24 24"><path d="M7 10l5 5 5-5z"/></svg>
                  </div>
                </div>
              </div>
              <div className="text-xs text-gray-500 whitespace-nowrap mt-1">
                {statementDetails?.date ? new Date(statementDetails.date).toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' }) : ""}
              </div>
            </div>

            {/* Email Body */}
            <div className="flex-1 min-h-0 px-8 pb-8">
              {statementDetails?.html ? (
                <div className="w-full h-full rounded-lg border border-slate-200 shadow-sm bg-white overflow-hidden">
                  <iframe 
                    srcDoc={statementDetails.html} 
                    className="w-full h-full border-0" 
                    title="Email Content" 
                    sandbox=""
                  />
                </div>
              ) : (
                <div className="w-full h-full rounded-lg border border-slate-200 bg-slate-50 p-6 overflow-y-auto">
                  <p className="whitespace-pre-wrap text-sm text-slate-800 font-sans leading-relaxed">{statementDetails?.snippet || "No email context available."}</p>
                </div>
              )}
            </div>
          </div>

          {/* Right Side: Password Input */}
          <div className="p-8 flex flex-col h-full overflow-y-auto bg-background">
            <DialogHeader className="mb-8">
              <DialogTitle id="password-dialog-title" className="flex items-center gap-2 text-2xl font-semibold tracking-tight text-foreground">
                <Lock className="w-5 h-5 text-destructive" aria-hidden="true" />
                Password Required
              </DialogTitle>
              <DialogDescription id="password-dialog-desc" className="text-sm mt-2 text-muted-foreground leading-relaxed">
                The uploaded statement is encrypted. Please provide the PDF password to continue processing.
              </DialogDescription>
            </DialogHeader>

            <div className="flex-1 flex flex-col justify-center">
              <div className="bg-card p-6 rounded-xl border border-border shadow-sm space-y-4">
                <div className="space-y-3">
                  <Label htmlFor="pdf-password" className="text-sm font-medium text-foreground">PDF Password</Label>
                  <div className="relative">
                    <Input
                      id="pdf-password"
                      type={showPassword ? "text" : "password"}
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
                      className="h-11 pr-10 border-input focus-visible:ring-ring transition-shadow"
                    />
                    <button
                      type="button"
                      onClick={() => setShowPassword(!showPassword)}
                      className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground focus:outline-none focus-visible:ring-2 focus-visible:ring-ring rounded-sm transition-colors"
                      aria-label={showPassword ? "Hide password" : "Show password"}
                    >
                      {showPassword ? (
                        <EyeOff className="h-5 w-5" aria-hidden="true" />
                      ) : (
                        <Eye className="h-5 w-5" aria-hidden="true" />
                      )}
                    </button>
                  </div>
                  {passwordError && (
                    <p id="password-error" role="alert" className="text-sm text-destructive flex items-center gap-1.5 mt-2 animate-in fade-in slide-in-from-top-1">
                      <AlertTriangle className="w-4 h-4" aria-hidden="true" />
                      {passwordError}
                    </p>
                  )}
                </div>

                <DialogFooter className="pt-2 grid grid-cols-2 gap-3 sm:space-x-0">
                  <Button variant="outline" onClick={close} aria-label="Cancel password entry" className="h-11 w-full border-input text-foreground hover:bg-accent hover:text-accent-foreground transition-colors">
                    Cancel
                  </Button>
                  <Button onClick={submitPassword} disabled={!password.trim() || isSubmitting} aria-label="Submit PDF password" className="h-11 w-full bg-primary hover:bg-primary/90 text-primary-foreground shadow-sm transition-all">
                    {isSubmitting ? 'Unlocking…' : 'Unlock & Parse'}
                  </Button>
                </DialogFooter>
              </div>
            </div>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
