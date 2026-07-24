import { AlertTriangle, Lock, RefreshCw, Trash2, Loader2 } from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { useToast } from '@/hooks/use-toast';
import { getErrorToast } from '@/lib/errorMapping';
import type { UnprocessedStatementEntry } from '@/lib/ipc';
import { useUnprocessedStatements } from '@/hooks/queries/useUnprocessedStatements';
import { useRetryUnprocessedStatement } from '@/hooks/mutations/useRetryUnprocessedStatement';
import { useDiscardUnprocessedStatement } from '@/hooks/mutations/useDiscardUnprocessedStatement';
import { useGlobalState } from '@/lib/GlobalStateContext';

interface UnprocessedItemsQueueProps {
  onEnterPassword: (statementId: string) => void;
}

const GROUPS: {
  key: 'awaiting_password' | 'pending_retry' | 'failed';
  label: string;
  badgeVariant: 'destructive' | 'secondary' | 'outline';
}[] = [
  { key: 'awaiting_password', label: 'Awaiting Password', badgeVariant: 'destructive' },
  { key: 'pending_retry', label: 'Pending Retry', badgeVariant: 'secondary' },
  { key: 'failed', label: 'Failed', badgeVariant: 'outline' },
];

/**
 * TASK-FE-012 (Doc 30): grouped by status (the real TASK-STMT-010 3-bucket
 * backend: awaiting_password/pending_retry/failed) with retry/discard
 * actions. `failure_reason` is always the structured, enum-like string the
 * backend writes at creation time (never a raw exception message —
 * guaranteed by `statements_discard`'s own doc comment) — rendered as-is,
 * never a stack trace.
 */
export default function UnprocessedItemsQueue({ onEnterPassword }: UnprocessedItemsQueueProps) {
  const { toast } = useToast();
  const { data: groups } = useUnprocessedStatements();
  const { openReviewModal } = useGlobalState();
  const retry = useRetryUnprocessedStatement();
  const discard = useDiscardUnprocessedStatement();

  const total = groups
    ? groups.awaiting_password.length +
      groups.pending_retry.length +
      groups.failed.length +
      groups.awaiting_review.length
    : 0;

  if (!groups || total === 0) return null;

  const handleRetry = (item: UnprocessedStatementEntry, groupKey: string) => {
    if (groupKey === 'awaiting_password') {
      onEnterPassword(item.statement_id);
      return;
    }
    retry.mutate(item.statement_id, {
      onSuccess: () =>
        toast({ title: 'Retrying', description: `${item.filename} queued for another attempt.` }),
      onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }),
    });
  };

  const handleDiscard = (item: UnprocessedStatementEntry) => {
    discard.mutate(item.statement_id, {
      onSuccess: () =>
        toast({ title: 'Discarded', description: `${item.filename} removed from the queue.` }),
      onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }),
    });
  };

  return (
    <Card className="border-amber-500/30 bg-amber-500/5">
      <CardHeader className="pb-3">
        <CardTitle className="flex items-center gap-2 text-amber-700">
          <AlertTriangle className="w-5 h-5" aria-hidden="true" />
          <span>Unprocessed Statements</span> <span>({total})</span>
        </CardTitle>
        <CardDescription>
          These statements require action before they can be processed.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-5">
        {groups.awaiting_review.length > 0 && (
          <div aria-label="Awaiting Review">
            <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-2">
              Awaiting Review ({groups.awaiting_review.length})
            </h3>
            <div className="space-y-2">
              {groups.awaiting_review.map((item) => (
                <div
                  key={item.draft_id}
                  className="flex items-center justify-between p-3 rounded-md bg-background border border-border"
                >
                  <div className="min-w-0">
                    <p className="text-sm font-medium truncate">
                      {item.issuer_name
                        ? `${item.issuer_name} •••${item.masked_identifier ?? '????'}`
                        : 'Statement ready for review'}
                    </p>
                  </div>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => openReviewModal(item.draft_id)}
                    aria-label={`Review ${item.issuer_name ?? 'statement'}`}
                  >
                    Review
                  </Button>
                </div>
              ))}
            </div>
          </div>
        )}
        {GROUPS.map((group) => {
          const items = groups[group.key];
          if (items.length === 0) return null;
          return (
            <div key={group.key} aria-label={group.label}>
              <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-2">
                {group.label} ({items.length})
              </h3>
              <div className="space-y-2">
                {items.map((item) => (
                  <div
                    key={item.statement_id}
                    className="flex items-center justify-between p-3 rounded-md bg-background border border-border"
                  >
                    <div className="min-w-0">
                      <p className="text-sm font-medium truncate">
                        {item.filename || 'Unknown file'}
                      </p>
                      {item.failure_reason && (
                        <p className="text-xs text-muted-foreground truncate">
                          {item.failure_reason}
                        </p>
                      )}
                    </div>
                    <div className="flex items-center gap-2 shrink-0">
                      <Badge variant={group.badgeVariant} className="text-xs">
                        {group.key === 'awaiting_password' && (
                          <Lock className="w-3 h-3 mr-1" aria-hidden="true" />
                        )}
                        {item.failure_type || group.label}
                      </Badge>
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => handleRetry(item, group.key)}
                        disabled={retry.isPending}
                        aria-label={`Retry ${item.filename}`}
                      >
                        {retry.isPending ? (
                          <Loader2 className="w-3 h-3 mr-1 animate-spin" aria-hidden="true" />
                        ) : (
                          <RefreshCw className="w-3 h-3 mr-1" aria-hidden="true" />
                        )}
                        {group.key === 'awaiting_password' ? 'Enter Password' : 'Retry'}
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        className="text-red-700 hover:text-red-700"
                        onClick={() => handleDiscard(item)}
                        disabled={discard.isPending}
                        aria-label={`Discard ${item.filename}`}
                      >
                        <Trash2 className="w-3 h-3" aria-hidden="true" />
                      </Button>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          );
        })}
      </CardContent>
    </Card>
  );
}
