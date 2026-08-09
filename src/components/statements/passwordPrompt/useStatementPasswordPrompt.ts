import { useCallback, useState, useEffect } from 'react';
import { API } from '@/lib/ipc';
import { useGlobalState } from '@/lib/GlobalStateContext';

/**
 * Derived from the IPC call that produces it rather than restated, so a change
 * to the `awaiting_password` shape surfaces here as a type error.
 */
export type StatementEmailContext = Awaited<
  ReturnType<typeof API.statements.listUnprocessed>
>['awaiting_password'][number];

function errorText(error: unknown): string {
  return typeof error === 'string' ? error : ((error as { message?: string })?.message ?? '');
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
      // The draft that eventually stages for this unlock reuses
      // pendingStatementId as its id (stage_parse_pipeline reuses whatever id
      // it's handed) — registering it here is what lets GlobalStateContext's
      // statement_staged listener recognize it and auto-open the review modal.
      watchDraftOrigin(pendingStatementId);
      // I9 fix (pre-existing): the backend resolves (never throws) for both
      // wrong-password and max-attempts-exceeded outcomes — `status`, not
      // promise rejection, is what distinguishes them.
      const result = await API.statements.submitPassword(
        pendingStatementId,
        pendingInstrumentId,
        password
      );

      if (result.status === 'unlocked') {
        // `statements_submit_password` runs staging synchronously and always
        // reuses `pendingStatementId` as the draft id for this path, so open
        // the review modal now rather than waiting on the statement_staged
        // event (which may have already fired during this same await).
        close();
        openReviewModal(result.draft_id || pendingStatementId);
        onUnlocked();
      } else if (result.status === 'awaiting_instrument_confirmation') {
        // Password was correct, but the bank/card/type couldn't be identified.
        // The Instrument Confirmation modal is already opening via its own
        // event listener — close silently so this one doesn't sit on top
        // showing a false "Incorrect password".
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
