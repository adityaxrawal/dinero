import React, { createContext, useContext } from 'react';
import type { ConnectedAccountInfo, ProcessingProgressPayload, ScanProgressPayload, StatementRecord } from './ipc';
import { useScanState, type ScanStatus } from './globalState/useScanState';
import { useStatementState } from './globalState/useStatementState';

/**
 * React Context carrying the long-lived scan and statement state.
 *
 * This is the one piece of state that genuinely needs a Provider rather than a
 * Zustand store, because it is built from hooks: both halves subscribe to Tauri
 * backend events and own timers and effects, which only run inside the React
 * tree. The Provider is what gives those subscriptions a single lifetime, so a
 * mailbox scan is tracked once for the whole app rather than once per component
 * that happens to care about it.
 *
 * The context value is simply the two hooks' results merged, and the interface
 * below is their combined surface written out explicitly -- long, but it makes
 * the contract reviewable and keeps consumers type-checked against it.
 */
export type { ScanStatus };

interface GlobalStateContextType {
  // Scan window, status, progress and lifecycle -- owned by useScanState.
  scanStartDate: string;
  setScanStartDate: React.Dispatch<React.SetStateAction<string>>;
  scanEndDate: string;
  setScanEndDate: React.Dispatch<React.SetStateAction<string>>;
  scanStatus: ScanStatus;
  setScanStatus: React.Dispatch<React.SetStateAction<ScanStatus>>;
  scanProgress: ScanProgressPayload | null;
  setScanProgress: React.Dispatch<React.SetStateAction<ScanProgressPayload | null>>;
  scanError: string | null;
  setScanError: React.Dispatch<React.SetStateAction<string | null>>;
  scanStartedAt: number | null;
  scanFinishedAt: number | null;
  handleCancelScan: () => Promise<void>;
  resetScan: () => void;

  // Gmail accounts available to scan, and the action that starts a scan.
  connectedAccounts: ConnectedAccountInfo[];
  setConnectedAccounts: React.Dispatch<React.SetStateAction<ConnectedAccountInfo[]>>;
  refreshConnectedAccounts: () => Promise<void>;
  handleStartScan: () => Promise<void>;

  statementHistory: StatementRecord[];
  statementLoading: boolean;
  loadStatementHistory: () => Promise<void>;

  batchProgress: { parsed: number; total: number; etaSeconds: number } | null;
  setBatchProgress: React.Dispatch<
    React.SetStateAction<{ parsed: number; total: number; etaSeconds: number } | null>
  >;

  // Password prompt, raised when the backend hits an encrypted statement PDF
  // and needs a passphrase before it can continue parsing.
  passwordModalOpen: boolean;
  setPasswordModalOpen: React.Dispatch<React.SetStateAction<boolean>>;
  pendingStatementId: string | null;
  setPendingStatementId: React.Dispatch<React.SetStateAction<string | null>>;
  pendingInstrumentId: string;
  setPendingInstrumentId: React.Dispatch<React.SetStateAction<string>>;
  openPasswordModal: (statementId: string, instrumentId?: string) => void;
  closePasswordModal: () => void;

  // Instrument prompt, raised when a statement cannot be attributed to a known
  // account. The hint fields carry what extraction did manage to recover, so
  // the dialog can pre-fill rather than asking from scratch.
  instrumentModalOpen: boolean;
  pendingInstrumentStatementId: string | null;
  pendingInstrumentFilename: string;
  pendingInstrumentIssuerHint: string;
  pendingInstrumentReason: string;
  closeInstrumentModal: () => void;

  // Draft review flow, where extracted transactions are confirmed before being
  // committed. watchDraftOrigin ties a draft back to the statement that
  // produced it, so the modal can open automatically once parsing finishes.
  reviewModalOpen: boolean;
  activeDraftId: string | null;
  processingProgress: ProcessingProgressPayload | null;
  openReviewModal: (draftId: string) => void;
  closeReviewModal: () => void;
  watchDraftOrigin: (originId: string) => void;
}

// Defaults to undefined rather than a stub value, so the hook below can tell a
// missing Provider apart from a legitimately empty state.
const GlobalStateContext = createContext<GlobalStateContextType | undefined>(undefined);

/**
 * Read the global state, throwing if used outside the Provider.
 *
 * Failing loudly is deliberate: every consumer needs live scan state, and
 * returning a silent default would surface as a component that simply never
 * updates -- far harder to diagnose than an explicit error at first render.
 */
// eslint-disable-next-line react-refresh/only-export-components
export const useGlobalState = () => {
  const context = useContext(GlobalStateContext);
  if (!context) throw new Error('useGlobalState must be used within a GlobalStateProvider');
  return context;
};

/**
 * Mounts the scan and statement hooks once and publishes their combined state.
 *
 * Must sit high enough in the tree to outlive any screen that starts work,
 * since unmounting it tears down the underlying event subscriptions and would
 * abandon an in-flight scan.
 */
export const GlobalStateProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const scan = useScanState();
  const statements = useStatementState();

  // Note this object is rebuilt on every render, so consumers re-render whenever
  // either hook updates. That is the intended behaviour here -- scan progress is
  // the main output and changes constantly, so memoising would buy nothing.
  const value: GlobalStateContextType = { ...scan, ...statements };

  return <GlobalStateContext.Provider value={value}>{children}</GlobalStateContext.Provider>;
};
