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
        setPasswordError('Incorrect password');
      }
    } catch (error: unknown) {
      setPasswordError(
        errorText(error).toLowerCase().includes('session has expired')
          ? 'Session expired. Please re-upload the file.'
          : 'Incorrect password'
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
