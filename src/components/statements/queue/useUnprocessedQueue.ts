/**
 * Loads the unprocessed queue and exposes retry and discard actions.
 */
import { useState } from 'react';
import type { StatementReparseProgress, UnprocessedStatementEntry } from '@/lib/ipc';
import { useToast } from '@/hooks/use-toast';
import { getErrorToast } from '@/lib/errorMapping';
import { useUnprocessedStatements } from '@/hooks/queries/useUnprocessedStatements';
import { useRetryUnprocessedStatement } from '@/hooks/mutations/useRetryUnprocessedStatement';
import { useDiscardUnprocessedStatement } from '@/hooks/mutations/useDiscardUnprocessedStatement';
import { useReparseAllStatements } from '@/hooks/mutations/useReparseAllStatements';
import { useIpcListen } from '@/hooks/useIpcListen';
import { useGlobalState } from '@/lib/GlobalStateContext';
import { entryLabel, type GroupKey } from './queueGroups';

/** Loads the unprocessed queue with retry and discard actions. */
export function useUnprocessedQueue(onEnterPassword: (statementId: string) => void) {
  const { toast } = useToast();
  const { openInstrumentModal } = useGlobalState();
  const { data: groups } = useUnprocessedStatements();
  const retry = useRetryUnprocessedStatement();
  const discard = useDiscardUnprocessedStatement();
  const reparseAll = useReparseAllStatements();
  const [progress, setProgress] = useState<StatementReparseProgress | null>(null);

  useIpcListen<StatementReparseProgress>('statement_reparse_progress', setProgress);

  const reviewable = groups?.awaiting_review ?? [];
  const actionable = groups
    ? (groups.awaiting_instrument?.length ?? 0) +
      groups.awaiting_password.length +
      groups.pending_retry.length +
      groups.failed.length
    : 0;

  /** Re-runs extraction across every stored statement. */
  const handleReparseAll = () => {
    setProgress(null);
    reparseAll.mutate(undefined, {
      onSuccess: (result) => {
        const stuck = (result.still_locked ?? 0) + (result.failed ?? 0);
        toast({
          title: `${result.parsed ?? 0} of ${result.total} parsed`,
          description: stuck
            ? `${stuck} still need attention — a locked file needs its password stored in Settings.`
            : 'The queue is clear.',
        });
      },
      onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }),
    });
  };

  /** Retries one failed statement. */
  const handleRetry = (item: UnprocessedStatementEntry, groupKey: GroupKey) => {
    if (groupKey === 'awaiting_password') {
      onEnterPassword(item.statement_id);
      return;
    }
    // Opened from the queue, one statement at a time, so a second parked
    // statement can no longer overwrite the first (audit F8-1, F8-2).
    if (groupKey === 'awaiting_instrument') {
      openInstrumentModal(
        item.statement_id,
        item.filename,
        item.issuer_hint ?? '',
        item.failure_reason ?? ''
      );
      return;
    }
    retry.mutate(item.statement_id, {
      onSuccess: () =>
        toast({
          title: 'Retrying',
          description: `${entryLabel(item)} queued for another attempt.`,
        }),
      onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }),
    });
  };

  /** Permanently dismisses a failed statement. */
  const handleDiscard = (item: UnprocessedStatementEntry) => {
    discard.mutate(item.statement_id, {
      onSuccess: () =>
        toast({ title: 'Discarded', description: `${entryLabel(item)} removed from the queue.` }),
      onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }),
    });
  };

  return {
    groups,
    reviewable,
    actionable,
    total: actionable + reviewable.length,
    progress,
    isReparsing: reparseAll.isPending,
    isRetrying: retry.isPending,
    isDiscarding: discard.isPending,
    handleReparseAll,
    handleRetry,
    handleDiscard,
  };
}
