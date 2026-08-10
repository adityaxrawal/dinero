/**
 * Owns the three dialogs that interrupt statement processing.
 *
 * Each one exists because the backend hit something it cannot resolve alone and
 * needs the user: an encrypted PDF needs a password, an unattributable statement
 * needs an instrument, and extracted transactions need confirmation before being
 * committed. Each dialog is a small self-contained hook here, and the exported
 * hook merges all three into one object.
 *
 * The split matters because each dialog carries its own pending context -- which
 * statement, which draft -- that must be captured when it opens and cleared when
 * it closes, so that a subsequent open never inherits stale values.
 */
import { useState, useCallback, useRef } from 'react';
import type { ProcessingProgressPayload } from '@/lib/ipc';

/** Password prompt for an encrypted statement PDF. */
function usePasswordModal() {
  const [passwordModalOpen, setPasswordModalOpen] = useState(false);
  const [pendingStatementId, setPendingStatementId] = useState<string | null>(null);
  // 'UNKNOWN' rather than null: the backend accepts a sentinel here, and the
  // password flow can proceed before the instrument has been identified.
  const [pendingInstrumentId, setPendingInstrumentId] = useState<string>('UNKNOWN');

  // Clearing the statement id on close prevents a stale target from being
  // submitted if the dialog is reopened before a new one is set.
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

/**
 * Prompt shown when a statement cannot be matched to a known instrument.
 *
 * The filename, issuer hint and reason are carried through so the dialog can
 * explain why it is asking and pre-fill what extraction already inferred,
 * rather than presenting an empty form.
 */
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
 * Draft review dialog, where extracted transactions are confirmed.
 *
 * `watchedOriginIds` is the interesting piece: when the user imports a
 * statement, its id is registered here, and the event layer later consults this
 * set to decide whether the resulting draft should open the review dialog
 * automatically. That is what distinguishes a draft the user is actively waiting
 * on from one produced by a background scan, which must not steal focus.
 */
function useReviewModal() {
  const [reviewModalOpen, setReviewModalOpen] = useState(false);
  const [activeDraftId, setActiveDraftId] = useState<string | null>(null);
  const [processingProgress, setProcessingProgress] = useState<ProcessingProgressPayload | null>(
    null
  );

  // A ref, not state: this is read by event handlers and must never trigger a
  // re-render, since nothing renders from it.
  const watchedOriginIds = useRef<Set<string>>(new Set());

  const watchDraftOrigin = useCallback((originId: string) => {
    watchedOriginIds.current.add(originId);
  }, []);

  // Progress is reset on open so a newly opened dialog never briefly shows the
  // previous draft's percentage.
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
 * Merge the three dialogs into one flat object.
 *
 * Safe to spread because the field names across the three hooks are disjoint;
 * any future overlap would silently shadow, so new fields should stay prefixed
 * by their dialog.
 */
export function useStatementModals() {
  return {
    ...usePasswordModal(),
    ...useInstrumentGateModal(),
    ...useReviewModal(),
  };
}
