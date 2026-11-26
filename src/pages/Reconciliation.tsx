/**
 * The reconciliation work queue: unresolved clusters and unassigned transactions.
 *
 * Two distinct problems share this screen because both are the same kind of task
 * -- something the pipeline could not decide alone and needs a person for.
 */
import { OUTCOME } from '@/lib/outcomeVocabulary';
import { useState, useMemo } from 'react';
import { useSearchParams } from 'react-router-dom';
import { ShieldAlert, HelpCircle, Loader2, CheckCircle2, Layers, RefreshCw } from 'lucide-react';

import { useReconciliationClusters } from '@/hooks/queries/useReconciliationClusters';
import { useLlmQueueStatus } from '@/hooks/queries/useLlmQueueStatus';
import { useUnassignedTransactions } from '@/hooks/queries/useUnassignedTransactions';
import { recheckAllMessage, useRecheckAllUnassigned } from '@/hooks/mutations/useRecheckUnassigned';
import { useToast } from '@/hooks/use-toast';
import { getErrorToast } from '@/lib/errorMapping';
import ReconciliationInspector from '@/components/reconciliation/ReconciliationInspector';
import UnassignedInspector from '@/components/reconciliation/UnassignedInspector';
import { cn, formatRelativeDate, isOlderThanDays } from '@/lib/utils';
import { getUnassignedDisplayInfo } from './reconciliationDisplay';
import type { ClusterRecord, LlmState, UnassignedTransactionRecord } from '@/lib/ipc';
import { formatMoney, moneyTone } from '@/lib/formatMoney';
import LoadError from '@/components/common/LoadError';

const ROW_CLASS =
  'flex flex-col w-full text-left px-3.5 py-3 mx-2 rounded-xl transition-all max-w-[calc(100%-16px)] cursor-pointer select-none border';
const ROW_SELECTED = 'bg-emerald-ink text-champagne border-emerald-ink shadow-sm';
const ROW_IDLE =
  'bg-white/60 hover:bg-white text-emerald-ink border-emerald-ink/10 hover:border-emerald-ink/20';

const CLUSTER_TONES = {
  selected: {
    row: ROW_SELECTED,
    badge: 'bg-champagne/20 text-champagne',
    meta: 'text-champagne/70',
    age: '',
    merchant: 'text-white',
  },
  idle: {
    row: ROW_IDLE,
    badge: 'bg-amber-100 text-amber-800 border border-amber-200',
    meta: 'text-slate-400',
    age: 'text-red-500 font-semibold',
    merchant: 'text-emerald-ink',
  },
};

/** One unresolved cluster in the queue list. */
function ClusterListItem({
  cluster,
  isSelected,
  onSelect,
}: {
  cluster: ClusterRecord;
  isSelected: boolean;
  onSelect: () => void;
}) {
  const tone = CLUSTER_TONES[isSelected ? 'selected' : 'idle'];
  const incoming = cluster.members.find((m) => m.member_role === 'incoming');
  const isDebit = incoming?.direction === 'debit';
  const amountStr = incoming
    ? formatMoney(Math.abs(incoming.amount_minor), incoming.currency, incoming.direction)
    : null;
  const ageStr = cluster.created_at ? formatRelativeDate(cluster.created_at) : null;
  const isStale = cluster.created_at ? isOlderThanDays(cluster.created_at, 3) : false;
  const amountTone = isSelected ? 'text-champagne' : isDebit ? 'text-red-700' : 'text-emerald-700';

  return (
    <button
      type="button"
      onClick={onSelect}
      aria-current={isSelected ? 'true' : undefined}
      className={cn(ROW_CLASS, tone.row)}
    >
      <div className="flex items-center justify-between gap-2 mb-1.5 w-full">
        <span
          className={cn(
            'text-[11px] font-bold px-1.5 py-0.5 rounded-full uppercase tracking-wider',
            tone.badge
          )}
        >
          Ambiguous
        </span>
        <span
          className={cn('flex items-center gap-1.5 text-[11px] font-medium shrink-0', tone.meta)}
        >
          {ageStr && (
            <span
              className={cn(isStale && tone.age)}
              title={isStale ? 'Pending more than 3 days' : undefined}
            >
              {ageStr}
              {/* Said in words too, not only by colour (WCAG 1.4.1). */}
              {isStale && ' · overdue'}
            </span>
          )}
          <span>{cluster.members_count} entries</span>
        </span>
      </div>
      <p className={cn('text-[13px] font-bold truncate mb-0.5', tone.merchant)}>
        {incoming?.merchant || 'Match requires review'}
      </p>
      {amountStr && (
        <p className={cn('text-[12px] font-mono font-semibold', amountTone)}>{amountStr}</p>
      )}
    </button>
  );
}

/** One unassigned transaction in the queue list. */
function UnassignedListItem({
  item,
  isSelected,
  onSelect,
}: {
  item: UnassignedTransactionRecord;
  isSelected: boolean;
  onSelect: () => void;
}) {
  const info = getUnassignedDisplayInfo(item);

  return (
    <button
      type="button"
      onClick={onSelect}
      aria-current={isSelected ? 'true' : undefined}
      className={cn(ROW_CLASS, isSelected ? ROW_SELECTED : ROW_IDLE)}
    >
      <div className="flex items-center justify-between gap-2 mb-1.5 w-full">
        <div className="flex items-center gap-2 min-w-0">
          <div
            className={cn(
              'w-5 h-5 rounded-full flex items-center justify-center text-[11px] font-bold shrink-0',
              isSelected ? 'bg-champagne text-emerald-ink' : 'bg-emerald-ink/10 text-emerald-ink'
            )}
          >
            {info.avatarLetter}
          </div>
          <span
            className={cn(
              'text-[13px] font-bold truncate tracking-tight',
              isSelected ? 'text-white' : 'text-slate-900'
            )}
          >
            {info.name}
          </span>
        </div>

        <span
          className={cn(
            'text-[11px] font-bold px-1.5 py-0.5 rounded-full uppercase tracking-wider shrink-0',
            isSelected
              ? 'bg-champagne/20 text-champagne'
              : item.reason === 'issuer_name_not_found'
                ? 'bg-purple-100 text-purple-700 border border-purple-200'
                : 'bg-amber-100 text-amber-800 border border-amber-200'
          )}
        >
          {item.waiting_for_ai ? OUTCOME.waitingForAi : info.issueLabel}
        </span>
      </div>

      <p
        className={cn(
          'text-[11px] line-clamp-1 mb-2 font-normal',
          isSelected ? 'text-champagne/80' : 'text-slate-600'
        )}
      >
        {info.snippet}
      </p>

      <div className="flex items-center justify-between text-[11px] font-medium pt-1 border-t border-current/10 w-full">
        {info.amountStr ? (
          <span
            className={cn(
              'font-semibold font-mono text-[12px]',
              // Coloured by direction, not always green (audit F9-8).
              isSelected ? 'text-champagne' : moneyTone(item.direction)
            )}
          >
            {info.amountStr}
          </span>
        ) : (
          <span
            className={cn(
              'italic text-[11px]',
              isSelected ? 'text-champagne/60' : 'text-slate-400'
            )}
          >
            Amount missing
          </span>
        )}

        {info.dateStr && (
          <span className={cn('text-[11px]', isSelected ? 'text-champagne/70' : 'text-slate-400')}>
            {info.dateStr}
          </span>
        )}
      </div>
    </button>
  );
}

/**
 * What the emails held for the local AI are waiting on. Only says "reading"
 * when a model can actually run; otherwise says why not (audit F7-1).
 */
function LlmQueueLine({ pending, state }: { pending: number; state: LlmState }) {
  const emails = `${pending} ${pending === 1 ? 'email' : 'emails'}`;
  return (
    <p className="flex items-center gap-1.5 text-[11px] text-emerald-ink/70" role="status">
      {state === 'ready' ? (
        <>
          <Loader2 className="w-3 h-3 animate-spin" aria-hidden="true" />
          AI is reading {emails}. You can fill them in now, or wait for it.
        </>
      ) : state === 'no_model' ? (
        <span>
          {emails} {pending === 1 ? 'is' : 'are'} waiting for the local AI.{' '}
          <a href="#/settings?section=advanced" className="underline font-medium">
            Download a model
          </a>{' '}
          to read {pending === 1 ? 'it' : 'them'}.
        </span>
      ) : (
        <span>
          {emails} {pending === 1 ? 'is' : 'are'} waiting for the local AI, but this Mac
          doesn't have enough memory to run it.
        </span>
      )}
    </p>
  );
}

/** Shown when a queue section has nothing left to resolve. */
function EmptyQueueNotice({ message }: { message: string }) {
  return <p className="text-[12px] text-center p-4 text-emerald-ink/60">{message}</p>;
}

/** Switches between the clusters and unassigned queues, with counts. */
function SectionTabs({
  sections,
  currentSection,
  onSelect,
}: {
  sections: readonly { id: string; label: string; icon: typeof ShieldAlert; badge: number }[];
  currentSection: string;
  onSelect: (id: string) => void;
}) {
  return (
    <div className="flex gap-1 overflow-x-auto pb-1" role="tablist">
      {sections.map((section) => {
        const isActive = currentSection === section.id;
        return (
          <button
            key={section.id}
            role="tab"
            aria-selected={isActive}
            onClick={() => onSelect(section.id)}
            className={cn(
              'px-3 py-1.5 text-[12px] font-medium rounded-full transition-colors whitespace-nowrap flex items-center gap-1.5',
              isActive ? 'bg-emerald-ink text-champagne' : 'text-emerald-ink/70 hover:bg-emerald-ink/10'
            )}
          >
            <section.icon className="w-3.5 h-3.5" />
            {section.label}
            {section.badge > 0 && (
              <span
                className={cn(
                  'ml-1 px-1.5 py-0.5 rounded-full text-[11px] font-bold',
                  isActive ? 'bg-champagne/20 text-champagne' : 'bg-emerald-ink/10 text-emerald-ink'
                )}
              >
                {section.badge}
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}

/** Loading, error and all-caught-up states for the queue list. */
function QueueStatus({ isLoading, onRetry }: { isLoading: boolean; onRetry?: () => void }) {
  // A failed load must never read as "All Caught Up" (audit F9-3).
  if (onRetry) {
    return <LoadError what="the review queue" onRetry={onRetry} className="h-full" />;
  }
  if (isLoading) {
    return (
      <div className="flex flex-col items-center justify-center h-40 gap-2">
        <Loader2 className="w-4 h-4 animate-spin text-emerald-ink/50" />
        <span className="text-xs text-emerald-ink/50">Loading queue...</span>
      </div>
    );
  }
  return (
    <div className="flex flex-col items-center justify-center text-center p-8 h-full opacity-60">
      <div className="w-12 h-12 rounded-full flex items-center justify-center mb-4 bg-emerald-ink/10 text-emerald-ink">
        <CheckCircle2 className="w-6 h-6" />
      </div>
      <h3 className="text-sm font-semibold mb-1 text-emerald-ink">All Caught Up!</h3>
      <p className="text-[11px] text-emerald-ink">Nothing is waiting for your review.</p>
    </div>
  );
}

/** The queue column, rendering whichever section is selected. */
function QueueList({
  currentSection,
  clusters,
  unassigned,
  selectedClusterId,
  selectedUnassignedId,
  onSelectCluster,
  onSelectUnassigned,
}: {
  currentSection: string;
  clusters: ClusterRecord[];
  unassigned: UnassignedTransactionRecord[];
  selectedClusterId: string | null;
  selectedUnassignedId: string | null;
  onSelectCluster: (id: string | null) => void;
  onSelectUnassigned: (id: string | null) => void;
}) {
  if (currentSection === 'clusters') {
    if (clusters.length === 0) return <EmptyQueueNotice message="No pending clusters." />;
    return (
      <div className="flex flex-col gap-1.5">
        {clusters.map((cluster) => (
          <ClusterListItem
            key={cluster.id}
            cluster={cluster}
            isSelected={selectedClusterId === cluster.id}
            onSelect={() => onSelectCluster(selectedClusterId === cluster.id ? null : cluster.id)}
          />
        ))}
      </div>
    );
  }

  if (currentSection !== 'unassigned') return null;
  if (unassigned.length === 0) return <EmptyQueueNotice message="No unassigned transactions." />;
  return (
    <div className="flex flex-col gap-1.5">
      {unassigned.map((item) => (
        <UnassignedListItem
          key={item.id}
          item={item}
          isSelected={selectedUnassignedId === item.id}
          onSelect={() => onSelectUnassigned(selectedUnassignedId === item.id ? null : item.id)}
        />
      ))}
    </div>
  );
}

/** The inspector column, showing the selected item's resolution UI. */
function InspectorPane({
  currentSection,
  clusters,
  selectedCluster,
  selectedUnassigned,
  selectedClusterId,
  selectedUnassignedId,
  onSelectCluster,
  onSelectUnassigned,
}: {
  currentSection: string;
  clusters: ClusterRecord[];
  selectedCluster: ClusterRecord | undefined;
  selectedUnassigned: UnassignedTransactionRecord | undefined;
  selectedClusterId: string | null;
  selectedUnassignedId: string | null;
  onSelectCluster: (id: string | null) => void;
  onSelectUnassigned: (id: string | null) => void;
}) {
  // The last copy of the open entry, kept if it leaves the queue in the
  // background so the user's edits aren't lost (audit F9-10).
  const [lastSeen, setLastSeen] = useState<UnassignedTransactionRecord | undefined>(undefined);
  if (selectedUnassigned && selectedUnassigned !== lastSeen) setLastSeen(selectedUnassigned);
  const lastSelectedUnassigned = lastSeen?.id === selectedUnassignedId ? lastSeen : undefined;

  if (selectedClusterId && currentSection === 'clusters') {
    return (
      <div className="w-full h-full flex flex-col">
        <ReconciliationInspector
          cluster={selectedCluster}
          onClose={() => onSelectCluster(null)}
          inline={true}
          queueClusters={clusters}
          onNavigate={onSelectCluster}
        />
      </div>
    );
  }

  if (selectedUnassignedId && currentSection === 'unassigned') {
    return (
      <div className="w-full h-full flex flex-col">
        <UnassignedInspector
          record={selectedUnassigned ?? lastSelectedUnassigned}
          resolvedElsewhere={!selectedUnassigned && !!lastSelectedUnassigned}
          onClose={() => onSelectUnassigned(null)}
          inline={true}
        />
      </div>
    );
  }

  return <InspectorPlaceholder currentSection={currentSection} />;
}

/** Prompt shown when nothing is selected yet. */
function InspectorPlaceholder({ currentSection }: { currentSection: string }) {
  return (
    <div className="flex-1 flex flex-col items-center justify-center h-full opacity-30">
      <div className="w-12 h-12 border-2 border-emerald-ink rounded-xl mb-4 border-dashed flex items-center justify-center">
        <Layers className="w-6 h-6 text-emerald-ink" />
      </div>
      <p className="text-emerald-ink font-medium text-sm">
        {currentSection === 'clusters'
          ? 'Select a cluster to resolve'
          : 'Select an item to view details'}
      </p>
    </div>
  );
}

/**
 * The reconciliation work queue.
 *
 * Two-column layout: a queue of items needing a decision, and an inspector for
 * whichever is selected. Clusters and unassigned transactions share the screen
 * because both are the same kind of task -- something the pipeline could not
 * resolve alone.
 *
 * Both underlying queries always refetch on mount, so the queue never presents
 * work the user has already dealt with.
 */
export default function Reconciliation() {
  const [searchParams, setSearchParams] = useSearchParams();
  /** Switches between the clusters and unassigned queues. */
  const setSection = (section: string) => setSearchParams({ section });

  const clustersQuery = useReconciliationClusters();
  const unassignedQuery = useUnassignedTransactions();
  const { data: clusters = [], isLoading: clustersLoading } = clustersQuery;
  const { data: unassigned = [], isLoading: unassignedLoading } = unassignedQuery;
  // Without an explicit tab, open the one that has work (audit F9-13).
  const currentSection =
    searchParams.get('section') ||
    (clusters.length === 0 && unassigned.length > 0 ? 'unassigned' : 'clusters');
  const { data: llmQueue } = useLlmQueueStatus();
  const recheckAll = useRecheckAllUnassigned();
  const { toast } = useToast();
  const canRecheckAll = unassigned.some((u) => u.can_recheck === true);
  /** Re-checks every entry that keeps its email (current-issues #3). */
  const handleRecheckAll = () =>
    recheckAll.mutate(undefined, {
      onSuccess: (summary) => toast(recheckAllMessage(summary)),
      onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }),
    });

  const [selectedClusterId, setSelectedClusterId] = useState<string | null>(null);
  const [selectedUnassignedId, setSelectedUnassignedId] = useState<string | null>(null);

  const isLoading = clustersLoading || unassignedLoading;
  const loadFailed = clustersQuery.isError || unassignedQuery.isError;
  const retryLoad = () => {
    if (clustersQuery.isError) clustersQuery.refetch();
    if (unassignedQuery.isError) unassignedQuery.refetch();
  };
  const allCaughtUp =
    !isLoading && !loadFailed && clusters.length === 0 && unassigned.length === 0;

  const selectedCluster = useMemo(
    () => clusters.find((c) => c.id === selectedClusterId),
    [clusters, selectedClusterId]
  );

  const selectedUnassigned = useMemo(
    () => unassigned.find((u) => u.id === selectedUnassignedId),
    [unassigned, selectedUnassignedId]
  );

  const SECTIONS = [
    // One vocabulary with the sidebar's "Review Inbox" (audit F9-12).
    { id: 'clusters', label: 'Possible duplicates', icon: ShieldAlert, badge: clusters.length },
    { id: 'unassigned', label: 'Needs details', icon: HelpCircle, badge: unassigned.length },
  ] as const;

  return (
    <div className="flex h-full w-full overflow-hidden">
      <div
        className="flex-shrink-0 flex flex-col h-full border-r border-emerald-ink/20"
        style={{ width: '320px', backgroundColor: 'var(--bg-canvas)' }}
      >
        <div className="flex flex-col gap-3 px-4 py-3 flex-shrink-0 border-b border-emerald-ink/10">
          <div className="flex items-center justify-between">
            <h1 className="text-[14px] font-semibold text-emerald-ink tracking-tight">
              Review Inbox
            </h1>
            {currentSection === 'unassigned' && canRecheckAll && (
              <button
                type="button"
                onClick={handleRecheckAll}
                disabled={recheckAll.isPending}
                className="flex items-center gap-1 text-[11px] font-medium text-emerald-ink px-2 py-1 rounded-full hover:bg-emerald-ink/10 disabled:opacity-50"
                title="Run your learned patterns again on every entry, then the local AI"
              >
                <RefreshCw
                  className={cn('w-3 h-3', recheckAll.isPending && 'animate-spin')}
                  aria-hidden="true"
                />
                Re-check all
              </button>
            )}
          </div>
          {llmQueue && llmQueue.pending > 0 && <LlmQueueLine {...llmQueue} />}

          <SectionTabs sections={SECTIONS} currentSection={currentSection} onSelect={setSection} />
        </div>

        <div className="flex-1 overflow-y-auto px-1 py-2">
          {loadFailed || isLoading || allCaughtUp ? (
            <QueueStatus isLoading={isLoading} {...(loadFailed ? { onRetry: retryLoad } : {})} />
          ) : (
            <div>
              <QueueList
                currentSection={currentSection}
                clusters={clusters}
                unassigned={unassigned}
                selectedClusterId={selectedClusterId}
                selectedUnassignedId={selectedUnassignedId}
                onSelectCluster={setSelectedClusterId}
                onSelectUnassigned={setSelectedUnassignedId}
              />
            </div>
          )}
        </div>
      </div>

      <div className="flex-1 h-full bg-champagne relative overflow-hidden flex flex-col justify-center">
        <InspectorPane
          currentSection={currentSection}
          clusters={clusters}
          selectedCluster={selectedCluster}
          selectedUnassigned={selectedUnassigned}
          selectedClusterId={selectedClusterId}
          selectedUnassignedId={selectedUnassignedId}
          onSelectCluster={setSelectedClusterId}
          onSelectUnassigned={setSelectedUnassignedId}
        />
      </div>
    </div>
  );
}
