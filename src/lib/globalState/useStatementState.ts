import { useStatementModals } from './useStatementModals';
import { useStatementEvents } from './useStatementEvents';

/**
 * Everything the statements pipeline pushes at the UI: parse outcomes, the
 * password prompt, the instrument gate, and the staged-draft review modal.
 * All of it is global because the events fire whether or not the user is on
 * the Statements page (e.g. during a background historical scan).
 */
export function useStatementState() {
  const modals = useStatementModals();
  const events = useStatementEvents(modals);

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
