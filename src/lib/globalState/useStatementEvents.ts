import { useState, useEffect, useCallback, useRef } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { API, type ProcessingProgressPayload, type StatementRecord } from '@/lib/ipc';
import { useToast } from '@/hooks/use-toast';
import type { useStatementModals } from './useStatementModals';
import { batchSummaryToast, emptyBatchOutcome, type BatchOutcome } from './batchSummary';

type Modals = ReturnType<typeof useStatementModals>;

/**
 * Every statements-pipeline event the backend can emit, and the history list
 * they invalidate. Toasts are aggregated during a tracked batch and debounced
 * for password prompts, both of which can fire dozens of times in a burst
 * during a historical scan.
 */
export function useStatementEvents(modals: Modals) {
  const { toast } = useToast();
  const [statementHistory, setStatementHistory] = useState<StatementRecord[]>([]);
  const [statementLoading, setStatementLoading] = useState(true);
  const [batchProgress, setBatchProgress] = useState<{
    parsed: number;
    total: number;
    etaSeconds: number;
  } | null>(null);

  // TASK-RT-008 (Doc 30): while a `statement_batch_progress`-tracked batch
  // (10+ files) is in flight, individual statement_parsed/statement_parse_failed
  // toasts are suppressed and tallied here instead, aggregating into one
  // summary toast on batch completion ("8/10 imported, 2 failed") rather
  // than one toast per file.
  const batchOutcomesRef = useRef<BatchOutcome | null>(null);

  // Doc 2026-07-26 mail scan performance: `statement_password_required` can
  // fire dozens of times in a burst during a historical scan -- these
  // accumulate the count between debounced toast flushes.
  const pendingPasswordCountRef = useRef(0);
  const passwordToastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

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
          batchOutcomesRef.current ??= emptyBatchOutcome();

          if (parsed >= total) {
            const summary = batchSummaryToast(batchOutcomesRef.current, total);
            batchOutcomesRef.current = null;
            if (summary) toast(summary);
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
        modals.openInstrumentModal(
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
          if (event.payload.draft_id && modals.watchedOriginIds.current.has(event.payload.draft_id)) {
            modals.setProcessingProgress(event.payload);
          }
        }
      );

      unlistenStaged = await listen<{ draft_id: string; origin: string }>(
        'statement_staged',
        (event) => {
          const { draft_id } = event.payload;
          if (modals.watchedOriginIds.current.has(draft_id)) {
            modals.watchedOriginIds.current.delete(draft_id);
            modals.openReviewModal(draft_id);
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
  }, [loadStatementHistory, toast, modals]);

  return {
    statementHistory,
    statementLoading,
    loadStatementHistory,
    batchProgress,
    setBatchProgress,
  };
}
