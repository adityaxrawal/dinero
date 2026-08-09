import { useState, useCallback, useRef } from 'react';
import type { ProcessingProgressPayload } from '@/lib/ipc';

/** The PDF password prompt, raised by `statement_password_required`. */
function usePasswordModal() {
  const [passwordModalOpen, setPasswordModalOpen] = useState(false);
  const [pendingStatementId, setPendingStatementId] = useState<string | null>(null);
  const [pendingInstrumentId, setPendingInstrumentId] = useState<string>('UNKNOWN');

  const closePasswordModal = useCallback(() => {
    setPasswordModalOpen(false);
    setPendingStatementId(null);
  }, []);

  const openPasswordModal = useCallback((statementId: string, instrumentId = 'UNKNOWN') => {
    setPendingStatementId(statementId);
    setPendingInstrumentId(instrumentId);
    setPasswordModalOpen(true);
  }, []);

  return {
    passwordModalOpen,
    setPasswordModalOpen,
    pendingStatementId,
    setPendingStatementId,
    pendingInstrumentId,
    setPendingInstrumentId,
    openPasswordModal,
    closePasswordModal,
  };
}

/** The Statement Instrument Gate confirmation modal (C2). */
function useInstrumentGateModal() {
  const [instrumentModalOpen, setInstrumentModalOpen] = useState(false);
  const [pendingInstrumentStatementId, setPendingInstrumentStatementId] = useState<string | null>(
    null
  );
  const [pendingInstrumentFilename, setPendingInstrumentFilename] = useState('');
  const [pendingInstrumentIssuerHint, setPendingInstrumentIssuerHint] = useState('');
  const [pendingInstrumentReason, setPendingInstrumentReason] = useState('');

  const openInstrumentModal = useCallback(
    (statementId: string, filename: string, issuerHint: string, reason: string) => {
      setPendingInstrumentStatementId(statementId);
      setPendingInstrumentFilename(filename);
      setPendingInstrumentIssuerHint(issuerHint);
      setPendingInstrumentReason(reason);
      setInstrumentModalOpen(true);
    },
    []
  );

  const closeInstrumentModal = useCallback(() => {
    setInstrumentModalOpen(false);
    setPendingInstrumentStatementId(null);
  }, []);

  return {
    instrumentModalOpen,
    pendingInstrumentStatementId,
    pendingInstrumentFilename,
    pendingInstrumentIssuerHint,
    pendingInstrumentReason,
    openInstrumentModal,
    closeInstrumentModal,
  };
}

/**
 * The staged-draft review modal. Only drafts whose origin was watched (i.e. a
 * user-initiated upload or unlock) auto-open; background-staged ones queue.
 */
function useReviewModal() {
  const [reviewModalOpen, setReviewModalOpen] = useState(false);
  const [activeDraftId, setActiveDraftId] = useState<string | null>(null);
  const [processingProgress, setProcessingProgress] = useState<ProcessingProgressPayload | null>(
    null
  );
  const watchedOriginIds = useRef<Set<string>>(new Set());

  const watchDraftOrigin = useCallback((originId: string) => {
    watchedOriginIds.current.add(originId);
  }, []);

  const openReviewModal = useCallback((draftId: string) => {
    setActiveDraftId(draftId);
    setProcessingProgress(null);
    setReviewModalOpen(true);
  }, []);

  const closeReviewModal = useCallback(() => {
    setReviewModalOpen(false);
    setActiveDraftId(null);
    setProcessingProgress(null);
  }, []);

  return {
    reviewModalOpen,
    activeDraftId,
    processingProgress,
    setProcessingProgress,
    openReviewModal,
    closeReviewModal,
    watchDraftOrigin,
    watchedOriginIds,
  };
}

/**
 * The three modals the statements pipeline can raise. Global because their
 * triggering events fire whether or not the user is on the Statements page.
 */
export function useStatementModals() {
  return {
    ...usePasswordModal(),
    ...useInstrumentGateModal(),
    ...useReviewModal(),
  };
}
