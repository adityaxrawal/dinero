import { useState } from 'react';
import {
  AlertTriangle,
  CheckCircle2,
  FileWarning,
  Loader2,
  Lock,
  RefreshCw,
  RotateCw,
  Trash2,
} from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { useToast } from '@/hooks/use-toast';
import { getErrorToast } from '@/lib/errorMapping';
import type { StatementReparseProgress, UnprocessedStatementEntry } from '@/lib/ipc';
import { useUnprocessedStatements } from '@/hooks/queries/useUnprocessedStatements';
import { useRetryUnprocessedStatement } from '@/hooks/mutations/useRetryUnprocessedStatement';
import { useDiscardUnprocessedStatement } from '@/hooks/mutations/useDiscardUnprocessedStatement';
import { useReparseAllStatements } from '@/hooks/mutations/useReparseAllStatements';
import { useIpcListen } from '@/hooks/useIpcListen';
import { useGlobalState } from '@/lib/GlobalStateContext';

interface UnprocessedItemsQueueProps {
  onEnterPassword: (statementId: string) => void;
}

type GroupKey = 'awaiting_password' | 'pending_retry' | 'failed';

/**
 * Issue #7: grouped by *what the user has to do about it*, not by the
 * backend's status enum. "pending_retry" and "failed" are two different
 * internal states that ask the same thing of a person — try it again — so
 * they sit together, while a locked PDF is the only group that genuinely
 * needs something the app cannot supply.
 */
const GROUPS: {
  key: GroupKey;
  label: string;
  hint: string;
  icon: typeof Lock;
  action: string;
}[] = [
  {
    key: 'awaiting_password',
    label: 'Needs a password',
    hint: 'No stored password opens these. Add one and they parse automatically next time.',
    icon: Lock,
    action: 'Enter Password',
  },
  {
    key: 'pending_retry',
    label: 'Waiting to retry',
    hint: 'Interrupted part-way through. Re-parsing usually clears these.',
    icon: RotateCw,
    action: 'Retry',
  },
  {
    key: 'failed',
    label: "Couldn't be read",
    hint: 'The pipeline could not extract anything usable from these files.',
    icon: FileWarning,
    action: 'Retry',
  },
];

export default function UnprocessedItemsQueue({ onEnterPassword }: UnprocessedItemsQueueProps) {
  const { toast } = useToast();
  const { data: groups } = useUnprocessedStatements();
  const { openReviewModal } = useGlobalState();
  const retry = useRetryUnprocessedStatement();
  const discard = useDiscardUnprocessedStatement();
  const reparseAll = useReparseAllStatements();
  const [progress, setProgress] = useState<StatementReparseProgress | null>(null);

  useIpcListen<StatementReparseProgress>('statement_reparse_progress', setProgress);

  const reviewable = groups?.awaiting_review ?? [];
  const actionable = groups
    ? groups.awaiting_password.length + groups.pending_retry.length + groups.failed.length
    : 0;
  const total = actionable + reviewable.length;

  if (!groups || total === 0) {
    return (
      <div className="rounded-xl border border-[#064E3B]/10 bg-[#F8E7C9]/50 p-10 text-center">
        <CheckCircle2 className="mx-auto mb-3 h-8 w-8 text-[#064E3B]/40" aria-hidden="true" />
        <p className="text-sm font-medium text-[#064E3B]">Nothing needs your attention</p>
        <p className="mt-1 text-[13px] text-[#064E3B]/60">
          Statements that need a password or fail to parse will collect here.
        </p>
      </div>
    );
  }

  const isReparsing = reparseAll.isPending;
  const percent =
    progress && progress.total > 0 ? Math.round((progress.processed / progress.total) * 100) : 0;

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

  const handleRetry = (item: UnprocessedStatementEntry, groupKey: GroupKey) => {
    if (groupKey === 'awaiting_password') {
      onEnterPassword(item.statement_id);
      return;
    }
    retry.mutate(item.statement_id, {
      onSuccess: () =>
        toast({ title: 'Retrying', description: `${label(item)} queued for another attempt.` }),
      onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }),
    });
  };

  const handleDiscard = (item: UnprocessedStatementEntry) => {
    discard.mutate(item.statement_id, {
      onSuccess: () =>
        toast({ title: 'Discarded', description: `${label(item)} removed from the queue.` }),
      onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }),
    });
  };

  return (
    <div className="space-y-4">
      {/* ── Bulk action bar ────────────────────────────────────────────── */}
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-[#064E3B]/10 bg-[#F8E7C9]/50 p-4">
        <div className="min-w-0">
          <p className="flex items-center gap-2 text-sm font-semibold text-[#064E3B]">
            <AlertTriangle className="h-4 w-4 text-amber-600" aria-hidden="true" />
            {total} {total === 1 ? 'statement needs' : 'statements need'} attention
          </p>
          <p className="mt-0.5 text-[13px] text-[#064E3B]/60">
            Re-parsing retries every one of them against your stored passwords.
          </p>
        </div>
        <Button
          onClick={handleReparseAll}
          disabled={isReparsing || actionable === 0}
          className="shrink-0 bg-[#064E3B] text-[#F8E7C9] hover:bg-[#064E3B]/90"
        >
          {isReparsing ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" aria-hidden="true" />
          ) : (
            <RefreshCw className="mr-2 h-4 w-4" aria-hidden="true" />
          )}
          {isReparsing ? 'Re-parsing…' : 'Re-parse All'}
        </Button>
      </div>

      {/* ── Progress ───────────────────────────────────────────────────── */}
      {progress && !progress.done && (
        <div className="rounded-xl border border-[#064E3B]/10 bg-[#F8E7C9]/50 p-4">
          <div
            className="h-1.5 w-full overflow-hidden rounded-full bg-[#064E3B]/10"
            role="progressbar"
            aria-valuenow={percent}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-label="Re-parse progress"
          >
            <div
              className="h-full rounded-full bg-[#064E3B] transition-all duration-300"
              style={{ width: `${percent}%` }}
            />
          </div>
          <p className="mt-2 text-[13px] text-[#064E3B]/70">
            {progress.processed} of {progress.total} checked
          </p>
        </div>
      )}

      {/* ── Ready to review ────────────────────────────────────────────── */}
      {reviewable.length > 0 && (
        <section
          aria-label="Ready to review"
          className="rounded-xl border border-[#064E3B]/10 bg-[#F8E7C9]/50 overflow-hidden"
        >
          <GroupHeader
            icon={CheckCircle2}
            label="Ready to review"
            count={reviewable.length}
            hint="Parsed successfully — confirm the details to import them."
          />
          <div className="divide-y divide-[#064E3B]/5">
            {reviewable.map((item) => (
              <div key={item.draft_id} className="flex items-center justify-between gap-3 p-4">
                <p className="min-w-0 truncate font-mono text-[13px] font-medium text-[#064E3B]">
                  {item.issuer_name
                    ? `${item.issuer_name} •••${item.masked_identifier ?? '????'}`
                    : 'Statement ready for review'}
                </p>
                <Button
                  variant="outline"
                  size="sm"
                  className="shrink-0 border-[#064E3B]/20 text-[#064E3B]"
                  onClick={() => openReviewModal(item.draft_id)}
                  aria-label={`Review ${item.issuer_name ?? 'statement'}`}
                >
                  Review
                </Button>
              </div>
            ))}
          </div>
        </section>
      )}

      {/* ── Actionable groups ──────────────────────────────────────────── */}
      {GROUPS.map((group) => {
        const items = groups[group.key];
        if (items.length === 0) return null;
        return (
          <section
            key={group.key}
            aria-label={group.label}
            className="rounded-xl border border-[#064E3B]/10 bg-[#F8E7C9]/50 overflow-hidden"
          >
            <GroupHeader
              icon={group.icon}
              label={group.label}
              count={items.length}
              hint={group.hint}
            />
            <div className="divide-y divide-[#064E3B]/5">
              {items.map((item) => (
                <div key={item.statement_id} className="flex items-center justify-between gap-3 p-4">
                  <div className="min-w-0">
                    {/* Issue #9: the derived `HDFCBANKXXXX1234JUN2026` name.
                        Monospaced so the fixed-width segments line up down
                        the column and a mismatched card is easy to spot. */}
                    <p className="truncate font-mono text-[13px] font-medium text-[#064E3B]">
                      {label(item)}
                    </p>
                    {item.display_name && item.filename && (
                      <p className="truncate text-xs text-[#064E3B]/50">{item.filename}</p>
                    )}
                    {item.failure_reason && (
                      <p className="mt-0.5 truncate text-xs text-[#064E3B]/60">
                        {item.failure_reason}
                      </p>
                    )}
                  </div>
                  <div className="flex shrink-0 items-center gap-2">
                    {item.failure_type && (
                      <Badge
                        variant="outline"
                        className="hidden border-[#064E3B]/20 text-xs text-[#064E3B]/70 sm:inline-flex"
                      >
                        {item.failure_type}
                      </Badge>
                    )}
                    <Button
                      variant="outline"
                      size="sm"
                      className="border-[#064E3B]/20 text-[#064E3B]"
                      onClick={() => handleRetry(item, group.key)}
                      disabled={retry.isPending || isReparsing}
                      aria-label={`${group.action} for ${label(item)}`}
                    >
                      {group.action}
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      className="text-red-700 hover:text-red-700"
                      onClick={() => handleDiscard(item)}
                      disabled={discard.isPending || isReparsing}
                      aria-label={`Discard ${label(item)}`}
                    >
                      <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          </section>
        );
      })}
    </div>
  );
}

function GroupHeader({
  icon: Icon,
  label,
  count,
  hint,
}: {
  icon: typeof Lock;
  label: string;
  count: number;
  hint: string;
}) {
  return (
    <div className="border-b border-[#064E3B]/10 bg-[#064E3B]/[0.03] px-4 py-3">
      <h3 className="flex items-center gap-2 text-sm font-semibold text-[#064E3B]">
        <Icon className="h-4 w-4" aria-hidden="true" />
        {label} ({count})
      </h3>
      <p className="mt-0.5 text-xs text-[#064E3B]/60">{hint}</p>
    </div>
  );
}

/**
 * Issue #9: the backend declines to invent a name when it cannot identify the
 * issuer, in which case the filename the bank itself chose is the more
 * informative label.
 */
function label(item: UnprocessedStatementEntry): string {
  return item.display_name || item.filename || 'Unknown file';
}
