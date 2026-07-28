import React, { createContext, useContext, useState, useEffect, useRef, useCallback } from 'react';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import {
  API,
  ConnectedAccountInfo,
  ProcessingProgressPayload,
  ScanProgressPayload,
  StatementRecord,
} from './ipc';
import { useToast } from '@/hooks/use-toast';

interface GlobalStateContextType {
  // Scan State
  scanStartDate: string;
  setScanStartDate: React.Dispatch<React.SetStateAction<string>>;
  scanEndDate: string;
  setScanEndDate: React.Dispatch<React.SetStateAction<string>>;
  scanStatus: 'idle' | 'running' | 'done' | 'error' | 'cancelled';
  setScanStatus: React.Dispatch<
    React.SetStateAction<'idle' | 'running' | 'done' | 'error' | 'cancelled'>
  >;
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
  const [scanStatus, setScanStatus] = useState<
    'idle' | 'running' | 'done' | 'error' | 'cancelled'
  >('idle');
  const [scanProgress, setScanProgress] = useState<ScanProgressPayload | null>(null);
  const [scanError, setScanError] = useState<string | null>(null);
  const [scanStartedAt, setScanStartedAt] = useState<number | null>(null);
  const [scanFinishedAt, setScanFinishedAt] = useState<number | null>(null);
  const unlistenScanRef = useRef<(() => void) | null>(null);
  const [connectedAccounts, setConnectedAccounts] = useState<ConnectedAccountInfo[]>([]);
  const [batchProgress, setBatchProgress] = useState<{
    parsed: number;
    total: number;
    etaSeconds: number;
  } | null>(null);

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
      mandate_events_found: 0,
      non_financial: 0,
      errors: 0,
      pending_enrichment: 0,
    });
    setScanError(null);
    setScanStartedAt(Date.now());
    setScanFinishedAt(null);

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
        setScanFinishedAt(Date.now());
        if (unlistenScanRef.current) {
          unlistenScanRef.current();
          unlistenScanRef.current = null;
        }
      });

      const unlistenError = await listen<{ error: string }>('scan_failed', (event) => {
        setScanStatus('error');
        setScanError(event.payload.error);
        setScanFinishedAt(Date.now());
        if (unlistenScanRef.current) {
          unlistenScanRef.current();
          unlistenScanRef.current = null;
        }
      });

      const unlistenCancelled = await listen<ScanProgressPayload>('scan_cancelled', (event) => {
        setScanStatus('cancelled');
        setScanProgress(event.payload);
        setScanFinishedAt(Date.now());
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
        unlistenCancelled();
      };

      await API.ingestion.startHistoricalScan(
        primaryConnectedAccount.account_id,
        scanStartDate,
        scanEndDate
      );
    } catch (err: unknown) {
      setScanStatus('error');
      setScanError(err instanceof Error ? err.message : String(err));
      setScanFinishedAt(Date.now());
    }
  };

  const handleCancelScan = async () => {
    if (!primaryConnectedAccount) return;
    try {
      await API.ingestion.cancelScan(primaryConnectedAccount.account_id);
    } catch (err: unknown) {
      setScanError(err instanceof Error ? err.message : String(err));
      throw err;
    }
  };

  const resetScan = () => {
    setScanStatus('idle');
    setScanProgress(null);
    setScanStartedAt(null);
    setScanFinishedAt(null);
  };

  // ----- Statements State -----
  const [statementHistory, setStatementHistory] = useState<StatementRecord[]>([]);
  const [statementLoading, setStatementLoading] = useState(true);

  const [passwordModalOpen, setPasswordModalOpen] = useState(false);
  const [pendingStatementId, setPendingStatementId] = useState<string | null>(null);
  const [pendingInstrumentId, setPendingInstrumentId] = useState<string>('UNKNOWN');

  // ----- Statement Instrument Gate confirmation modal (C2) -----
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

  // ----- Statement Review Modal -----
  const [reviewModalOpen, setReviewModalOpen] = useState(false);
  const [activeDraftId, setActiveDraftId] = useState<string | null>(null);
  const [processingProgress, setProcessingProgress] = useState<ProcessingProgressPayload | null>(
    null
  );
  const watchedOriginIds = useRef<Set<string>>(new Set());

  // TASK-RT-008 (Doc 30): while a `statement_batch_progress`-tracked batch
  // (10+ files) is in flight, individual statement_parsed/statement_parse_failed
  // toasts are suppressed and tallied here instead, aggregating into one
  // summary toast on batch completion ("8/10 imported, 2 failed") rather
  // than one toast per file.
  const batchOutcomesRef = useRef<{
    succeeded: number;
    failed: number;
    failureReasons: string[];
  } | null>(null);

  // Doc 2026-07-26 mail scan performance: `statement_password_required` can
  // fire dozens of times in a burst during a historical scan -- these
  // accumulate the count between debounced toast flushes.
  const pendingPasswordCountRef = useRef(0);
  const passwordToastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

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

  const closePasswordModal = useCallback(() => {
    setPasswordModalOpen(false);
    setPendingStatementId(null);
  }, []);

  const openPasswordModal = useCallback((statementId: string, instrumentId = 'UNKNOWN') => {
    setPendingStatementId(statementId);
    setPendingInstrumentId(instrumentId);
    setPasswordModalOpen(true);
  }, []);

  useEffect(() => {
    loadStatementHistory();

    let unlistenParsed: UnlistenFn;
    let unlistenPassword: UnlistenFn;
    let unlistenFailed: UnlistenFn;
    let unlistenDuplicate: UnlistenFn;
    let unlistenInstrumentConfirmation: UnlistenFn;
    let unlistenBatchProgress: UnlistenFn;
    let unlistenProgress: UnlistenFn;
    let unlistenStaged: UnlistenFn;

    const setupListeners = async () => {
      // Doc 30 TASK-RT-008: the real payload is
      // `{statement_id, instrument_id, issuer_name, rows_extracted}` --
      // previously only `statement_id` was emitted at all, so this always
      // showed a generic, contentless toast. `test_single_upload_shows_summary_toast`.
      unlistenParsed = await listen<{
        statement_id: string;
        instrument_id?: string;
        issuer_name?: string | null;
        rows_extracted?: number;
      }>('statement_parsed', (event) => {
        loadStatementHistory();
        if (batchOutcomesRef.current) {
          batchOutcomesRef.current.succeeded += 1;
          return;
        }
        const { issuer_name, rows_extracted } = event.payload;
        const subject = issuer_name || 'Statement';
        const count = rows_extracted ?? 0;
        toast({
          title: 'Statement Parsed',
          description: `${subject} statement parsed — ${count} transaction${count === 1 ? '' : 's'} found.`,
        });
      });

      // Doc 2026-07-26 mail scan performance: a locked statement PDF
      // encountered mid-scan must never hijack the screen --
      // `resolve_statement_password`'s backend comment notes this event
      // "can fire dozens of times in a burst during a historical scan."
      // Previously every single firing called `openPasswordModal`,
      // repeatedly reassigning it to whichever PDF fired last. The row is
      // already durably queryable via `UnprocessedItemsQueue`'s "Awaiting
      // Password" section (`Statements.tsx`'s `onEnterPassword` already
      // opens this same modal on demand) -- this listener now only
      // refreshes that queue and shows one debounced, batched toast.
      unlistenPassword = await listen<{ statementId: string; instrumentId?: string }>(
        'statement_password_required',
        () => {
          loadStatementHistory();
          pendingPasswordCountRef.current += 1;
          if (passwordToastTimerRef.current) {
            clearTimeout(passwordToastTimerRef.current);
          }
          passwordToastTimerRef.current = setTimeout(() => {
            const count = pendingPasswordCountRef.current;
            pendingPasswordCountRef.current = 0;
            toast({
              title: 'Password Required',
              description: `${count} statement${count === 1 ? '' : 's'} need${count === 1 ? 's' : ''} a password — check Action Needed.`,
            });
          }, 2000);
        }
      );

      // Real payload is `{reason, filename}` -- previously discarded in
      // favor of a hardcoded generic message.
      unlistenFailed = await listen<{ reason?: string; filename?: string }>(
        'statement_parse_failed',
        (event) => {
          loadStatementHistory();
          const { reason, filename } = event.payload;
          if (batchOutcomesRef.current) {
            batchOutcomesRef.current.failed += 1;
            if (reason) batchOutcomesRef.current.failureReasons.push(reason);
            return;
          }
          toast({
            title: 'Parse Failed',
            description: filename
              ? `Failed to parse ${filename}${reason ? `: ${reason}` : ''}.`
              : 'Failed to extract data from statement.',
            variant: 'destructive',
            actionTo: '/statements',
            actionLabel: 'View retry panel',
          });
        }
      );

      unlistenDuplicate = await listen('statement_duplicate_rejected', () => {
        loadStatementHistory();
        toast({
          title: 'Duplicate Statement',
          description: 'This statement has already been processed.',
          variant: 'destructive',
        });
      });

      unlistenBatchProgress = await listen<{ parsed: number; total: number; eta_seconds: number }>(
        'statement_batch_progress',
        (event) => {
          const { parsed, total, eta_seconds } = event.payload;

          // First tick of a fresh batch -- start tallying individual
          // statement_parsed/statement_parse_failed events instead of
          // toasting each one.
          if (!batchOutcomesRef.current) {
            batchOutcomesRef.current = { succeeded: 0, failed: 0, failureReasons: [] };
          }

          if (parsed >= total) {
            // Doc 30 TASK-RT-008 `test_batch_upload_aggregates_into_single_summary`:
            // one aggregate toast on batch completion
            // ("8/10 imported, 2 failed due to password") instead of one
            // toast per file.
            const outcome = batchOutcomesRef.current;
            batchOutcomesRef.current = null;
            if (outcome && (outcome.succeeded > 0 || outcome.failed > 0)) {
              const mostCommonReason = outcome.failureReasons[0];
              toast({
                title: 'Batch Import Complete',
                description:
                  outcome.failed > 0
                    ? `${outcome.succeeded}/${total} imported, ${outcome.failed} failed${mostCommonReason ? ` (${mostCommonReason})` : ''}.`
                    : `${outcome.succeeded}/${total} imported.`,
                variant: outcome.failed > 0 ? 'destructive' : 'default',
              });
            }
          }

          // Cleared once the last statement in the batch finishes -- the
          // intake round trip (statements_upload) returns long before this,
          // so this is the only signal for "still working through the
          // 5-concurrent-parser cap" during that window.
          setBatchProgress(parsed >= total ? null : { parsed, total, etaSeconds: eta_seconds });
        }
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
          payload.reason ??
            'The statement issuer or account number could not be read automatically.'
        );
      });

      unlistenProgress = await listen<ProcessingProgressPayload>(
        'statement_processing_progress',
        (event) => {
          if (event.payload.draft_id && watchedOriginIds.current.has(event.payload.draft_id)) {
            setProcessingProgress(event.payload);
          }
        }
      );

      unlistenStaged = await listen<{ draft_id: string; origin: string }>(
        'statement_staged',
        (event) => {
          const { draft_id } = event.payload;
          if (watchedOriginIds.current.has(draft_id)) {
            watchedOriginIds.current.delete(draft_id);
            openReviewModal(draft_id);
          } else {
            // Background-staged (email/historical scan) — not auto-opened.
            toast({
              title: 'Statement ready for review',
              description: 'Check the Awaiting Review queue on the Statements page.',
            });
          }
        }
      );
    };

    setupListeners();

    return () => {
      if (unlistenParsed) unlistenParsed();
      if (unlistenPassword) unlistenPassword();
      if (unlistenFailed) unlistenFailed();
      if (unlistenDuplicate) unlistenDuplicate();
      if (unlistenInstrumentConfirmation) unlistenInstrumentConfirmation();
      if (unlistenBatchProgress) unlistenBatchProgress();
      if (unlistenProgress) unlistenProgress();
      if (unlistenStaged) unlistenStaged();
    };
  }, [loadStatementHistory, toast, openPasswordModal, openInstrumentModal, openReviewModal]);

  const value: GlobalStateContextType = {
    scanStartDate,
    setScanStartDate,
    scanEndDate,
    setScanEndDate,
    scanStatus,
    setScanStatus,
    scanProgress,
    setScanProgress,
    scanError,
    setScanError,
    scanStartedAt,
    scanFinishedAt,
    connectedAccounts,
    setConnectedAccounts,
    refreshConnectedAccounts,
    handleStartScan,
    handleCancelScan,
    resetScan,

    statementHistory,
    statementLoading,
    loadStatementHistory,
    batchProgress,
    setBatchProgress,
    passwordModalOpen,
    setPasswordModalOpen,
    pendingStatementId,
    setPendingStatementId,
    pendingInstrumentId,
    setPendingInstrumentId,
    openPasswordModal,
    closePasswordModal,

    instrumentModalOpen,
    pendingInstrumentStatementId,
    pendingInstrumentFilename,
    pendingInstrumentIssuerHint,
    pendingInstrumentReason,
    closeInstrumentModal,

    reviewModalOpen,
    activeDraftId,
    processingProgress,
    openReviewModal,
    closeReviewModal,
    watchDraftOrigin,
  };

  return <GlobalStateContext.Provider value={value}>{children}</GlobalStateContext.Provider>;
};
