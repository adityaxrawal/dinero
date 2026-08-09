import { useCallback, useEffect, useState } from 'react';
import { API } from '@/lib/ipc';
import { getErrorMessage } from '@/lib/errorMapping';
import { useGlobalState } from '@/lib/GlobalStateContext';

/** The Statement Instrument Gate: which account a statement belongs to, asked
 *  only when the parser could not work it out. */
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
      // Resumes the same synchronous stage_parse_pipeline as the password
      // path — reuses pendingInstrumentStatementId as the draft id, so the
      // review modal can open directly with it, no event race.
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
