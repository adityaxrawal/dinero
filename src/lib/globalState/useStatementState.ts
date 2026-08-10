/**
 * Composition point for all statement-related global state.
 *
 * The work is split across two hooks with a one-way dependency: useStatementModals
 * owns which dialogs are open and what they are working on, while
 * useStatementEvents subscribes to backend statement events and drives those
 * dialogs in response -- a password prompt appearing because the Rust side
 * reported an encrypted PDF, for instance.
 *
 * This hook wires the two together and republishes them as a single flat object,
 * so consumers get one import and remain unaware of the internal split.
 */
import { useStatementModals } from './useStatementModals';
import { useStatementEvents } from './useStatementEvents';

/** Composes the statement modal and event hooks into one surface. */
export function useStatementState() {
  const modals = useStatementModals();

  // Events receive the modal controls so backend notifications can open and
  // close dialogs directly. The dependency runs strictly this way; the modal
  // hook knows nothing about events.
  const events = useStatementEvents(modals);

  // Event fields are spread first, then modal fields are listed explicitly.
  // The explicit list is the point: it keeps this surface an intentional,
  // reviewable contract rather than whatever the two hooks happen to expose.
  return {
    ...events,
    passwordModalOpen: modals.passwordModalOpen,
    setPasswordModalOpen: modals.setPasswordModalOpen,
    pendingStatementId: modals.pendingStatementId,
    setPendingStatementId: modals.setPendingStatementId,
    pendingInstrumentId: modals.pendingInstrumentId,
    setPendingInstrumentId: modals.setPendingInstrumentId,
    openPasswordModal: modals.openPasswordModal,
    closePasswordModal: modals.closePasswordModal,
    instrumentModalOpen: modals.instrumentModalOpen,
    pendingInstrumentStatementId: modals.pendingInstrumentStatementId,
    pendingInstrumentFilename: modals.pendingInstrumentFilename,
    pendingInstrumentIssuerHint: modals.pendingInstrumentIssuerHint,
    pendingInstrumentReason: modals.pendingInstrumentReason,
    closeInstrumentModal: modals.closeInstrumentModal,
    reviewModalOpen: modals.reviewModalOpen,
    activeDraftId: modals.activeDraftId,
    processingProgress: modals.processingProgress,
    openReviewModal: modals.openReviewModal,
    closeReviewModal: modals.closeReviewModal,
    watchDraftOrigin: modals.watchDraftOrigin,
  };
}
