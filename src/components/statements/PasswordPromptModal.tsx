/**
 * Prompts for the password of an encrypted statement PDF.
 *
 * Raised by the backend during parsing. The source email is shown alongside,
 * because banks commonly state the password rule in that very message.
 */
import { Dialog, DialogContent } from '@/components/ui/dialog';
import { useStatementPasswordPrompt } from './passwordPrompt/useStatementPasswordPrompt';
import EmailContextPane from './passwordPrompt/EmailContextPane';
import PasswordForm from './passwordPrompt/PasswordForm';

/**
 * Prompts for an encrypted statement's password.
 *
 * The source email is shown alongside, because banks commonly state the password
 * rule in that very message.
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
