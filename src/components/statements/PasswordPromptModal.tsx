import { Dialog, DialogContent } from '@/components/ui/dialog';
import { useStatementPasswordPrompt } from './passwordPrompt/useStatementPasswordPrompt';
import EmailContextPane from './passwordPrompt/EmailContextPane';
import PasswordForm from './passwordPrompt/PasswordForm';

/**
 * TASK-FE-012 (Doc 30): triggered by the backend's `statement_password_required`
 * event — GlobalStateContext owns that event listener + the 2.5-minute
 * countdown state (a genuinely global concern: the event can fire while the
 * user isn't even on the Statements page, e.g. during a background
 * historical scan), this component is purely the extracted modal UI over
 * that existing, already-correct state. Shows clear wrong-password retry
 * messaging.
 */
export default function PasswordPromptModal({ onUnlocked }: { onUnlocked: () => void }) {
  const prompt = useStatementPasswordPrompt(onUnlocked);

  return (
    <Dialog
      open={prompt.passwordModalOpen}
      onOpenChange={(open) => {
        if (!open) prompt.close();
      }}
    >
      <DialogContent
        className="sm:max-w-[1050px] w-[95vw] p-0 overflow-hidden flex flex-col max-h-[90vh] h-[750px]"
        aria-labelledby="password-dialog-title"
        aria-describedby="password-dialog-desc"
      >
        <div className="grid grid-cols-1 md:grid-cols-[minmax(0,1fr)_400px] flex-1 min-h-0 h-full">
          <EmailContextPane details={prompt.statementDetails} />
          <PasswordForm prompt={prompt} />
        </div>
      </DialogContent>
    </Dialog>
  );
}
