import React, { createContext, useContext, useState, useEffect, useRef, useCallback } from 'react';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { API, ConnectedAccountInfo, ScanProgressPayload, StatementRecord } from './ipc';
import { useToast } from '@/hooks/use-toast';

interface GlobalStateContextType {
  // Scan State
  scanStartDate: string;
  setScanStartDate: React.Dispatch<React.SetStateAction<string>>;
  scanEndDate: string;
  setScanEndDate: React.Dispatch<React.SetStateAction<string>>;
  scanStatus: 'idle' | 'running' | 'done' | 'error';
  setScanStatus: React.Dispatch<React.SetStateAction<'idle' | 'running' | 'done' | 'error'>>;
  scanProgress: ScanProgressPayload | null;
  setScanProgress: React.Dispatch<React.SetStateAction<ScanProgressPayload | null>>;
  scanError: string | null;
  setScanError: React.Dispatch<React.SetStateAction<string | null>>;
  
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
  setBatchProgress: React.Dispatch<React.SetStateAction<{ parsed: number; total: number; etaSeconds: number } | null>>;
  
  // Password Modal State (from Statements)
  passwordModalOpen: boolean;
  setPasswordModalOpen: React.Dispatch<React.SetStateAction<boolean>>;
  pendingStatementId: string | null;
  setPendingStatementId: React.Dispatch<React.SetStateAction<string | null>>;
  pendingInstrumentId: string;
  setPendingInstrumentId: React.Dispatch<React.SetStateAction<string>>;
  passwordTimeoutCountdown: number;
  openPasswordModal: (statementId: string, instrumentId?: string) => void;
  closePasswordModal: () => void;

  // Statement Instrument Gate confirmation modal (C2)
  instrumentModalOpen: boolean;
  pendingInstrumentStatementId: string | null;
  pendingInstrumentFilename: string;
  pendingInstrumentIssuerHint: string;
  pendingInstrumentReason: string;
  closeInstrumentModal: () => void;
}

const GlobalStateContext = createContext<GlobalStateContextType | undefined>(undefined);

export const useGlobalState = () => {
  const context = useContext(GlobalStateContext);
  if (!context) throw new Error("useGlobalState must be used within a GlobalStateProvider");
  return context;
};

const PASSWORD_TIMEOUT_SECONDS = 150;

export const GlobalStateProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const { toast } = useToast();

  // ----- Settings Scan State -----
  const [scanStartDate, setScanStartDate] = useState(() => {
    const d = new Date();
    d.setMonth(d.getMonth() - 1);
    return d.toISOString().split('T')[0];
  });
  const [scanEndDate, setScanEndDate] = useState(() => {
    return new Date().toISOString().split('T')[0];
  });
  const [scanStatus, setScanStatus] = useState<'idle' | 'running' | 'done' | 'error'>('idle');
  const [scanProgress, setScanProgress] = useState<ScanProgressPayload | null>(null);
  const [scanError, setScanError] = useState<string | null>(null);
  const unlistenScanRef = useRef<(() => void) | null>(null);
  const [connectedAccounts, setConnectedAccounts] = useState<ConnectedAccountInfo[]>([]);
  const [batchProgress, setBatchProgress] = useState<{ parsed: number; total: number; etaSeconds: number } | null>(null);

  const refreshConnectedAccounts = async () => {
    try {
      const accounts = await API.auth.listConnectedAccounts();
      setConnectedAccounts(accounts);
    } catch (err) {
      console.error('Failed to fetch connected accounts:', err);
    }
  };

  useEffect(() => {
    refreshConnectedAccounts();
    const interval = setInterval(refreshConnectedAccounts, 3000);
    return () => clearInterval(interval);
  }, []);

  // Doc 13 Flow 4.1's scan trigger predates multi-account support (Doc 03
  // §8.2) — scanning always targets the first-connected account; scanning a
  // specific one of several accounts is a Mail Scan UX enhancement outside
  // M02's scope (connecting/gating multiple accounts).
  const primaryConnectedAccount = connectedAccounts[0] ?? null;

  const handleStartScan = async () => {
    if (!primaryConnectedAccount) {
      alert('Please connect a Gmail account first.');
      return;
    }
    if (!scanStartDate || !scanEndDate) {
      alert('Please select both a start and end date.');
      return;
    }
    if (scanStartDate > scanEndDate) {
      alert('Start date must be before end date.');
      return;
    }

    setScanStatus('running');
    setScanProgress({
      account_id: primaryConnectedAccount.account_id,
      processed: 0,
      total: 0,
      transactions_found: 0,
      statements_found: 0,
      non_financial: 0,
      errors: 0
    });
    setScanError(null);

    try {
      if (unlistenScanRef.current) {
        unlistenScanRef.current();
      }

      const unlisten = await listen<ScanProgressPayload>('scan_progress', (event) => {
        setScanProgress(event.payload);
      });
      unlistenScanRef.current = unlisten;

      const unlistenDone = await listen<ScanProgressPayload>('scan_completed', (event) => {
        setScanStatus('done');
        setScanProgress(event.payload);
        if (unlistenScanRef.current) {
          unlistenScanRef.current();
          unlistenScanRef.current = null;
        }
      });
      
      const unlistenError = await listen<{ error: string }>('scan_failed', (event) => {
        setScanStatus('error');
        setScanError(event.payload.error);
        if (unlistenScanRef.current) {
          unlistenScanRef.current();
          unlistenScanRef.current = null;
        }
      });
      
      const prev = unlistenScanRef.current;
      unlistenScanRef.current = () => {
        if (prev) prev();
        unlistenDone();
        unlistenError();
      };
      
      await API.ingestion.startHistoricalScan(primaryConnectedAccount.account_id, scanStartDate, scanEndDate);
    } catch (err: any) {
      setScanStatus('error');
      setScanError(err?.message ?? String(err));
    }
  };

  // ----- Statements State -----
  const [statementHistory, setStatementHistory] = useState<StatementRecord[]>([]);
  const [statementLoading, setStatementLoading] = useState(true);
  
  const [passwordModalOpen, setPasswordModalOpen] = useState(false);
  const [pendingStatementId, setPendingStatementId] = useState<string | null>(null);
  const [pendingInstrumentId, setPendingInstrumentId] = useState<string>('UNKNOWN');
  const [passwordTimeoutCountdown, setPasswordTimeoutCountdown] = useState(PASSWORD_TIMEOUT_SECONDS);
  const countdownRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // ----- Statement Instrument Gate confirmation modal (C2) -----
  const [instrumentModalOpen, setInstrumentModalOpen] = useState(false);
  const [pendingInstrumentStatementId, setPendingInstrumentStatementId] = useState<string | null>(null);
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
    [],
  );

  const closeInstrumentModal = useCallback(() => {
    setInstrumentModalOpen(false);
    setPendingInstrumentStatementId(null);
  }, []);

  const loadStatementHistory = useCallback(async () => {
    setStatementLoading(true);
    try {
      const data = await API.statements.listHistory();
      setStatementHistory(data);
    } catch (e) {
      console.error('Failed to load statements', e);
    } finally {
      setStatementLoading(false);
    }
  }, []);

  const clearCountdown = useCallback(() => {
    if (countdownRef.current) {
      clearInterval(countdownRef.current);
      countdownRef.current = null;
    }
  }, []);

  const closePasswordModal = useCallback(() => {
    clearCountdown();
    setPasswordModalOpen(false);
    setPendingStatementId(null);
  }, [clearCountdown]);

  const startCountdown = useCallback(() => {
    clearCountdown();
    setPasswordTimeoutCountdown(PASSWORD_TIMEOUT_SECONDS);
    countdownRef.current = setInterval(() => {
      setPasswordTimeoutCountdown((prev) => {
        if (prev <= 1) {
          clearCountdown();
          setPasswordModalOpen(false);
          setPendingStatementId(null);
          toast({
            variant: 'destructive',
            title: 'Password Timeout',
            description: 'The 2.5-minute window to enter the password has expired.',
          });
          return PASSWORD_TIMEOUT_SECONDS;
        }
        return prev - 1;
      });
    }, 1000);
  }, [clearCountdown, toast]);

  const openPasswordModal = useCallback(
    (statementId: string, instrumentId = 'UNKNOWN') => {
      setPendingStatementId(statementId);
      setPendingInstrumentId(instrumentId);
      setPasswordModalOpen(true);
      startCountdown();
    },
    [startCountdown],
  );

  useEffect(() => {
    loadStatementHistory();

    let unlistenParsed: UnlistenFn;
    let unlistenPassword: UnlistenFn;
    let unlistenTimeout: UnlistenFn;
    let unlistenFailed: UnlistenFn;
    let unlistenDuplicate: UnlistenFn;
    let unlistenInstrumentConfirmation: UnlistenFn;
    let unlistenBatchProgress: UnlistenFn;

    const setupListeners = async () => {
      unlistenParsed = await listen('statement_parsed', () => {
        loadStatementHistory();
        toast({ title: 'Statement Parsed', description: 'Data successfully extracted.' });
      });

      unlistenPassword = await listen<{ statementId: string; instrumentId?: string }>(
        'statement_password_required',
        (event) => {
          const payload: any = event.payload;
          openPasswordModal(payload.statementId || payload.statement_id, payload.instrumentId || payload.instrument_id);
        },
      );

      unlistenTimeout = await listen<{ statementId: string }>(
        'statement_password_timeout',
        (_event) => {
          setPasswordModalOpen(false);
          loadStatementHistory();
        }
      );

      unlistenFailed = await listen('statement_parse_failed', () => {
        loadStatementHistory();
        toast({ title: 'Parse Failed', description: 'Failed to extract data from statement.', variant: 'destructive' });
      });

      unlistenDuplicate = await listen('statement_duplicate_rejected', () => {
        loadStatementHistory();
        toast({ title: 'Duplicate Statement', description: 'This statement has already been processed.', variant: 'destructive' });
      });

      unlistenBatchProgress = await listen<{ parsed: number; total: number; eta_seconds: number }>(
        'statement_batch_progress',
        (event) => {
          const { parsed, total, eta_seconds } = event.payload;
          // Cleared once the last statement in the batch finishes -- the
          // intake round trip (statements_upload) returns long before this,
          // so this is the only signal for "still working through the
          // 5-concurrent-parser cap" during that window.
          setBatchProgress(parsed >= total ? null : { parsed, total, etaSeconds: eta_seconds });
        },
      );

      unlistenInstrumentConfirmation = await listen<{
        statement_id: string;
        filename?: string;
        issuer?: string;
        reason?: string;
      }>('statement_instrument_confirmation_required', (event) => {
        const payload = event.payload;
        openInstrumentModal(
          payload.statement_id,
          payload.filename ?? '',
          payload.issuer ?? '',
          payload.reason ?? 'The statement issuer or account number could not be read automatically.',
        );
      });
    };

    setupListeners();

    return () => {
      if (unlistenParsed) unlistenParsed();
      if (unlistenPassword) unlistenPassword();
      if (unlistenTimeout) unlistenTimeout();
      if (unlistenFailed) unlistenFailed();
      if (unlistenDuplicate) unlistenDuplicate();
      if (unlistenInstrumentConfirmation) unlistenInstrumentConfirmation();
      if (unlistenBatchProgress) unlistenBatchProgress();
      clearCountdown();
    };
  }, [loadStatementHistory, toast, openPasswordModal, openInstrumentModal, clearCountdown]);


  const value: GlobalStateContextType = {
    scanStartDate, setScanStartDate,
    scanEndDate, setScanEndDate,
    scanStatus, setScanStatus,
    scanProgress, setScanProgress,
    scanError, setScanError,
    connectedAccounts, setConnectedAccounts,
    refreshConnectedAccounts,
    handleStartScan,

    statementHistory,
    statementLoading,
    loadStatementHistory,
    batchProgress, setBatchProgress,
    passwordModalOpen, setPasswordModalOpen,
    pendingStatementId, setPendingStatementId,
    pendingInstrumentId, setPendingInstrumentId,
    passwordTimeoutCountdown,
    openPasswordModal,
    closePasswordModal,

    instrumentModalOpen,
    pendingInstrumentStatementId,
    pendingInstrumentFilename,
    pendingInstrumentIssuerHint,
    pendingInstrumentReason,
    closeInstrumentModal,
  };

  return (
    <GlobalStateContext.Provider value={value}>
      {children}
    </GlobalStateContext.Provider>
  );
};
