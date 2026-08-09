import { CheckCircle2, Lock, Trash2 } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import type { UnprocessedStatementEntry } from '@/lib/ipc';
import { useGlobalState } from '@/lib/GlobalStateContext';
import { GROUPS, entryLabel, type GroupKey } from './queueGroups';
import type { useUnprocessedQueue } from './useUnprocessedQueue';

type Queue = ReturnType<typeof useUnprocessedQueue>;

const SECTION = 'rounded-xl border border-[#064E3B]/10 bg-[#F8E7C9]/50 overflow-hidden';

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

export function ReviewableSection({ items }: { items: Queue['reviewable'] }) {
  const { openReviewModal } = useGlobalState();

  return (
    <section aria-label="Ready to review" className={SECTION}>
      <GroupHeader
        icon={CheckCircle2}
        label="Ready to review"
        count={items.length}
        hint="Parsed successfully — confirm the details to import them."
      />
      <div className="divide-y divide-[#064E3B]/5">
        {items.map((item) => (
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
  );
}

function EntryRow({
  item,
  action,
  queue,
  groupKey,
}: {
  item: UnprocessedStatementEntry;
  action: string;
  queue: Queue;
  groupKey: GroupKey;
}) {
  const name = entryLabel(item);

  return (
    <div className="flex items-center justify-between gap-3 p-4">
      <div className="min-w-0">
        {/* Issue #9: the derived `HDFCBANKXXXX1234JUN2026` name. Monospaced so
            the fixed-width segments line up down the column and a mismatched
            card is easy to spot. */}
        <p className="truncate font-mono text-[13px] font-medium text-[#064E3B]">{name}</p>
        {item.display_name && item.filename && (
          <p className="truncate text-xs text-[#064E3B]/50">{item.filename}</p>
        )}
        {item.failure_reason && (
          <p className="mt-0.5 truncate text-xs text-[#064E3B]/60">{item.failure_reason}</p>
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
          onClick={() => queue.handleRetry(item, groupKey)}
          disabled={queue.isRetrying || queue.isReparsing}
          aria-label={`${action} for ${name}`}
        >
          {action}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          className="text-red-700 hover:text-red-700"
          onClick={() => queue.handleDiscard(item)}
          disabled={queue.isDiscarding || queue.isReparsing}
          aria-label={`Discard ${name}`}
        >
          <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
        </Button>
      </div>
    </div>
  );
}

export function ActionableGroups({ queue }: { queue: Queue }) {
  const { groups } = queue;
  if (!groups) return null;

  return (
    <>
      {GROUPS.map((group) => {
        const items = groups[group.key];
        if (items.length === 0) return null;
        return (
          <section key={group.key} aria-label={group.label} className={SECTION}>
            <GroupHeader
              icon={group.icon}
              label={group.label}
              count={items.length}
              hint={group.hint}
            />
            <div className="divide-y divide-[#064E3B]/5">
              {items.map((item) => (
                <EntryRow
                  key={item.statement_id}
                  item={item}
                  action={group.action}
                  queue={queue}
                  groupKey={group.key}
                />
              ))}
            </div>
          </section>
        );
      })}
    </>
  );
}
