import React, { createContext, useContext } from 'react';
import type { ConnectedAccountInfo, ProcessingProgressPayload, ScanProgressPayload, StatementRecord } from './ipc';
import { useScanState, type ScanStatus } from './globalState/useScanState';
import { useStatementState } from './globalState/useStatementState';

export type { ScanStatus };

interface GlobalStateContextType {
  // Scan State
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

  // Settings Connected Accounts (Doc 03 §8.2: up to 10 simultaneously connected)
  connectedAccounts: ConnectedAccountInfo[];
  setConnectedAccounts: React.Dispatch<React.SetStateAction<ConnectedAccountInfo[]>>;
  refreshConnectedAccounts: () => Promise<void>;
  handleStartScan: () => Promise<void>;

  // Statements State
  statementHistory: StatementRecord[];
  statementLoading: boolean;
  loadStatementHistory: () => Promise<void>;

  // TASK-FE-012 (Doc 30): "queued state for items beyond the backend's
  // 5-concurrent-parser cap" -- mirrors the real statement_batch_progress
  // event (queues.rs's BatchProgressTracker, batches over 10 files only).
  batchProgress: { parsed: number; total: number; etaSeconds: number } | null;
  setBatchProgress: React.Dispatch<
    React.SetStateAction<{ parsed: number; total: number; etaSeconds: number } | null>
  >;

  // Password Modal State (from Statements)
  passwordModalOpen: boolean;
  setPasswordModalOpen: React.Dispatch<React.SetStateAction<boolean>>;
  pendingStatementId: string | null;
  setPendingStatementId: React.Dispatch<React.SetStateAction<string | null>>;
  pendingInstrumentId: string;
  setPendingInstrumentId: React.Dispatch<React.SetStateAction<string>>;
  openPasswordModal: (statementId: string, instrumentId?: string) => void;
  closePasswordModal: () => void;

  // Statement Instrument Gate confirmation modal (C2)
  instrumentModalOpen: boolean;
  pendingInstrumentStatementId: string | null;
  pendingInstrumentFilename: string;
  pendingInstrumentIssuerHint: string;
  pendingInstrumentReason: string;
  closeInstrumentModal: () => void;

  // Statement Review Modal (staged extraction review, replaces the old
  // "toast + silent auto-commit" flow)
  reviewModalOpen: boolean;
  activeDraftId: string | null;
  processingProgress: ProcessingProgressPayload | null;
  openReviewModal: (draftId: string) => void;
  closeReviewModal: () => void;
  watchDraftOrigin: (originId: string) => void;
}

const GlobalStateContext = createContext<GlobalStateContextType | undefined>(undefined);

// eslint-disable-next-line react-refresh/only-export-components
export const useGlobalState = () => {
  const context = useContext(GlobalStateContext);
  if (!context) throw new Error('useGlobalState must be used within a GlobalStateProvider');
  return context;
};

export const GlobalStateProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const scan = useScanState();
  const statements = useStatementState();

  const value: GlobalStateContextType = { ...scan, ...statements };

  return <GlobalStateContext.Provider value={value}>{children}</GlobalStateContext.Provider>;
};
