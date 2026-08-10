import { useState, useEffect, useCallback, useRef } from 'react';
import { listen, type EventCallback, type UnlistenFn } from '@tauri-apps/api/event';
import { API, type ProcessingProgressPayload, type StatementRecord } from '@/lib/ipc';
import { useToast } from '@/hooks/use-toast';
import type { useStatementModals } from './useStatementModals';
import { batchSummaryToast, emptyBatchOutcome, type BatchOutcome } from './batchSummary';

/**
 * Subscribes to every statement-processing event and reacts to it.
 *
 * This is where backend statement activity becomes visible: history reloads,
 * toasts, and the dialogs that ask the user for a password or an instrument.
 *
 * Two pieces of debouncing shape the behaviour, both there to keep a bulk import
 * from producing a wall of notifications:
 *
 *   - Batch mode. Once a batch is in progress, per-statement successes and
 *     failures are tallied into a ref instead of toasted, and one summary is
 *     emitted when the batch completes.
 *   - Password coalescing. Each password-required event restarts a two-second
 *     timer, so ten encrypted PDFs produce a single "10 statements need a
 *     password" toast rather than ten separate ones.
 *
 * Counters live in refs rather than state deliberately: they are read inside
 * event handlers, they must not trigger renders, and a ref is not captured stale
 * by the long-lived closures registered below.
 */
type Modals = ReturnType<typeof useStatementModals>;

/** Subscribes to statement-processing events and reacts to them. */
export function useStatementEvents(modals: Modals) {
  const { toast } = useToast();

  const { openInstrumentModal, setProcessingProgress, openReviewModal, watchedOriginIds } = modals;
  const [statementHistory, setStatementHistory] = useState<StatementRecord[]>([]);
  const [statementLoading, setStatementLoading] = useState(true);
  const [batchProgress, setBatchProgress] = useState<{
    parsed: number;
    total: number;
    etaSeconds: number;
  } | null>(null);

  // Non-null exactly while a batch import is running. Its presence is the flag
  // that suppresses per-statement toasts in favour of a final summary.
  const batchOutcomesRef = useRef<BatchOutcome | null>(null);

  // Password-prompt coalescing: a running count plus the timer that flushes it.
  const pendingPasswordCountRef = useRef(0);
  const passwordToastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  /**
   * Reload statement history from the backend.
   *
   * Called after any event that could have changed a statement's state, so the
   * list stays authoritative rather than being patched incrementally from event
   * payloads.
   */
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

  useEffect(() => {
    loadStatementHistory();

    const unlisteners: UnlistenFn[] = [];
    let cancelled = false;

    /**
     * Attach one listener, guarding against a race.
     *
     * `listen` is async, so the effect can be cleaned up while a subscription is
     * still resolving. The `cancelled` check immediately releases any listener
     * that arrives after teardown, which would otherwise leak and keep firing
     * against an unmounted component.
     */
    const register = async <T>(event: string, handler: EventCallback<T>) => {
      const unlisten = await listen<T>(event, handler);
      if (cancelled) unlisten();
      else unlisteners.push(unlisten);
    };

    /** Registers every statement event listener. */
    const setupListeners = async () => {
      await register<{
        statement_id: string;
        instrument_id?: string;
        issuer_name?: string | null;
        rows_extracted?: number;
      }>('statement_parsed', (event) => {
        loadStatementHistory();
        // In batch mode, tally silently -- the summary toast reports the total.
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

      await register<{ statementId: string; instrumentId?: string }>(
        'statement_password_required',
        () => {
          loadStatementHistory();
          pendingPasswordCountRef.current += 1;
          // Restarting the timer on each event is what coalesces a burst: the
          // toast only fires once two quiet seconds have passed, by which point
          // the count covers the whole batch.
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

      await register<{ reason?: string; filename?: string }>('statement_parse_failed', (event) => {
        loadStatementHistory();
        const { reason, filename } = event.payload;
        // Batch mode again: record the failure and its reason for the summary
        // rather than interrupting with a toast per failed file.
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
      });

      await register('statement_duplicate_rejected', () => {
        loadStatementHistory();
        toast({
          title: 'Duplicate Statement',
          description: 'This statement has already been processed.',
          variant: 'destructive',
        });
      });

      await register<{ parsed: number; total: number; eta_seconds: number }>(
        'statement_batch_progress',
        (event) => {
          const { parsed, total, eta_seconds } = event.payload;

          // The first progress event is what enters batch mode; from here on the
          // per-statement handlers tally instead of toasting.
          batchOutcomesRef.current ??= emptyBatchOutcome();

          // Batch finished: emit the one summary and leave batch mode, so any
          // later single import toasts normally again.
          if (parsed >= total) {
            const summary = batchSummaryToast(batchOutcomesRef.current, total);
            batchOutcomesRef.current = null;
            if (summary) toast(summary);
          }

          // Null clears the progress bar on completion.
          setBatchProgress(parsed >= total ? null : { parsed, total, etaSeconds: eta_seconds });
        }
      );

      await register<{
        statement_id: string;
        filename?: string;
        issuer?: string;
        reason?: string;
      // Extraction could not attribute the statement to an account. The dialog
      // opens regardless of batch mode -- this needs an answer to proceed, so it
      // cannot be deferred to a summary.
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

      // Progress is only surfaced for drafts the user is actively waiting on;
      // background scans produce drafts too, and their progress must not
      // hijack whatever dialog is currently open.
      await register<ProcessingProgressPayload>('statement_processing_progress', (event) => {
        if (event.payload.draft_id && watchedOriginIds.current.has(event.payload.draft_id)) {
          setProcessingProgress(event.payload);
        }
      });

      // A draft is ready. If the user initiated this import, open the review
      // dialog immediately; otherwise point them at the queue rather than
      // interrupting what they are doing. The id is removed once consumed so a
      // later re-staging of the same draft no longer auto-opens.
      await register<{ draft_id: string; origin: string }>('statement_staged', (event) => {
        const { draft_id } = event.payload;
        if (watchedOriginIds.current.has(draft_id)) {
          watchedOriginIds.current.delete(draft_id);
          openReviewModal(draft_id);
        } else {
          toast({
            title: 'Statement ready for review',
            description: 'Check the Awaiting Review queue on the Statements page.',
          });
        }
      });
    };

    setupListeners();

    // Flip the guard first so any subscription still resolving is released on
    // arrival, then detach everything already registered.
    return () => {
      cancelled = true;
      for (const unlisten of unlisteners) unlisten();
    };
  }, [
    loadStatementHistory,
    toast,
    openInstrumentModal,
    setProcessingProgress,
    openReviewModal,
    watchedOriginIds,
  ]);

  return {
    statementHistory,
    statementLoading,
    loadStatementHistory,
    batchProgress,
    setBatchProgress,
  };
}
