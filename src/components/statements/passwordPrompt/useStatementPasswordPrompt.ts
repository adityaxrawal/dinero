/**
 * State and submission for the password prompt, including retry handling.
 */
import { useCallback, useState, useEffect } from 'react';
import { API } from '@/lib/ipc';
import { useGlobalState } from '@/lib/GlobalStateContext';

export type StatementEmailContext = Awaited<
  ReturnType<typeof API.statements.listUnprocessed>
>['awaiting_password'][number];

/** Turns a failure into readable prompt text. */
function errorText(error: unknown): string {
  return typeof error === 'string' ? error : ((error as { message?: string })?.message ?? '');
}

/** State and submission for the password prompt, including retries. */
/**
 * Wrong-password message: names the file and says where banks state the
 * format, instead of a bare "Incorrect password" (current-issues #4). Generic
 * guidance only; nothing about the user is guessed.
 */
export function wrongPasswordMessage(name: string | null | undefined): string {
  const file = name ? `"${name}"` : 'this statement';
  return `That password didn't open ${file}. The bank's email or website usually says what it is made of (often part of your name with your date of birth, or digits of your card or account number).`;
}

export function useStatementPasswordPrompt(onUnlocked: () => void) {
  const {
    passwordModalOpen,
    pendingStatementId,
    pendingInstrumentId,
    closePasswordModal,
    watchDraftOrigin,
    openReviewModal,
  } = useGlobalState();

  const [password, setPassword] = useState('');
  const [passwordError, setPasswordError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [statementDetails, setStatementDetails] = useState<StatementEmailContext | null>(null);

  useEffect(() => {
    if (!passwordModalOpen || !pendingStatementId) {
      setStatementDetails(null);
      return;
    }
    API.statements
      .listUnprocessed()
      .then((groups) => {
        const found = groups.awaiting_password.find((s) => s.statement_id === pendingStatementId);
        if (found) setStatementDetails(found);
      })
      .catch((err) => {
        console.error('[PasswordPromptModal] Error fetching unprocessed statements:', err);
      });
  }, [passwordModalOpen, pendingStatementId]);

  /** Closes the prompt and clears entered text. */
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
      watchDraftOrigin(pendingStatementId);
      const result = await API.statements.submitPassword(
        pendingStatementId,
        pendingInstrumentId,
        password
      );

      if (result.status === 'unlocked') {
        close();
        openReviewModal(result.draft_id || pendingStatementId);
        onUnlocked();
      } else if (result.status === 'awaiting_instrument_confirmation') {
        close();
      } else {
        setPasswordError(wrongPasswordMessage(statementDetails?.display_name ?? statementDetails?.filename));
      }
    } catch (error: unknown) {
      setPasswordError(
        errorText(error).toLowerCase().includes('session has expired')
          ? 'Session expired. Please re-upload the file.'
          : wrongPasswordMessage(statementDetails?.display_name ?? statementDetails?.filename)
      );
    } finally {
      setIsSubmitting(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    pendingStatementId,
    pendingInstrumentId,
    password,
    onUnlocked,
    watchDraftOrigin,
    openReviewModal,
    statementDetails,
  ]);

  return {
    passwordModalOpen,
    password,
    setPassword,
    passwordError,
    setPasswordError,
    isSubmitting,
    statementDetails,
    close,
    submitPassword,
  };
}
