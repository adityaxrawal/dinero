/**
 * State and submission for the instrument gate dialog.
 */
import { useCallback, useEffect, useState } from 'react';
import { API } from '@/lib/ipc';
import { getErrorMessage } from '@/lib/errorMapping';
import { useGlobalState } from '@/lib/GlobalStateContext';

/** State and submission for the instrument gate dialog. */
export function useInstrumentGate(refresh: () => void) {
  const {
    instrumentModalOpen,
    pendingInstrumentStatementId,
    pendingInstrumentIssuerHint,
    closeInstrumentModal,
    openReviewModal,
  } = useGlobalState();

  const [issuer, setIssuer] = useState('');
  const [masked, setMasked] = useState('');
  const [type, setType] = useState('credit_card');
  const [error, setError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);

  useEffect(() => {
    if (instrumentModalOpen) {
      setIssuer(pendingInstrumentIssuerHint || '');
      setMasked('');
      setType('credit_card');
      setError(null);
    }
  }, [instrumentModalOpen, pendingInstrumentIssuerHint]);

  const submit = useCallback(async () => {
    if (!pendingInstrumentStatementId || !issuer.trim() || !masked.trim()) return;
    setIsSubmitting(true);
    setError(null);
    try {
      const result = await API.statements.confirmInstrument(
        pendingInstrumentStatementId,
        issuer.trim(),
        masked.trim(),
        type
      );
      closeInstrumentModal();
      openReviewModal(result.draft_id || pendingInstrumentStatementId);
      refresh();
    } catch (e) {
      setError(getErrorMessage(e, 'Could not process the statement with these details.'));
    } finally {
      setIsSubmitting(false);
    }
  }, [
    pendingInstrumentStatementId,
    issuer,
    masked,
    type,
    closeInstrumentModal,
    openReviewModal,
    refresh,
  ]);

  return {
    issuer,
    setIssuer,
    masked,
    setMasked,
    type,
    setType,
    error,
    setError,
    isSubmitting,
    submit,
  };
}
