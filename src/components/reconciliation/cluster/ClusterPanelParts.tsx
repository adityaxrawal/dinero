/**
 * Presentational pieces of the cluster resolution panel.
 */
import type { ReactNode } from 'react';
import { Loader2, GitMerge, Ban, Clock, SplitSquareHorizontal, ChevronLeft, ChevronRight } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { formatRelativeDate } from '@/lib/utils';
import type { ClusterRecord } from '@/lib/ipc';
import type { useResolveClusterActions } from '@/hooks/useResolveClusterActions';

type Actions = ReturnType<typeof useResolveClusterActions>;

/**
 * Pairs an action with a caption explaining its consequence.
 *
 * Used throughout the resolution panel, because each verdict does something
 * materially different to the data and the button label alone cannot convey it.
 */
function ActionWithCaption({ caption, children }: { caption: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      {children}
      <p className="text-[10px] text-center text-muted-foreground leading-snug px-1">{caption}</p>
    </div>
  );
}

/** Placeholder while a cluster loads. */
export function ClusterLoading() {
  return (
    <div className="flex items-center justify-center py-16 gap-2 text-sm text-muted-foreground">
      <Loader2 className="w-4 h-4 animate-spin" aria-hidden="true" /> Loading cluster details…
    </div>
  );
}

/**
 * Shown when a cluster id no longer resolves.
 *
 * Expected rather than exceptional: the cluster may have been resolved in
 * another view, or from a notification, since this URL is directly linkable.
 */
export function ClusterNotFound() {
  return (
    <div className="flex flex-col items-center justify-center py-16 gap-2 text-center">
      <p className="text-base font-semibold text-[#064E3B]">Cluster Not Found</p>
      <p className="text-sm text-muted-foreground">
        This cluster could not be found. It may already be resolved.
      </p>
    </div>
  );
}

/** Previous/next navigation through the cluster queue, for resolving in sequence. */
export function QueueNav({
  queueClusters,
  clusterId,
  onNavigate,
}: {
  queueClusters: ClusterRecord[];
  clusterId: string | undefined;
  onNavigate: ((clusterId: string) => void) | undefined;
}) {
  const index = queueClusters.findIndex((c) => c.id === clusterId);
  if (index < 0) return null;

  const prev = index > 0 ? queueClusters[index - 1] : null;
  const next = index < queueClusters.length - 1 ? queueClusters[index + 1] : null;
  const arrow =
    'w-6 h-6 flex items-center justify-center rounded-md hover:bg-[#064E3B]/10 disabled:opacity-30 disabled:hover:bg-transparent';

  return (
    <div className="flex items-center gap-1 text-[11px] font-medium text-[#064E3B]/60">
      <button
        type="button"
        className={arrow}
        onClick={() => prev && onNavigate?.(prev.id)}
        disabled={!prev}
        aria-label="Previous cluster"
      >
        <ChevronLeft className="w-4 h-4" />
      </button>
      <span>
        {index + 1} of {queueClusters.length}
      </span>
      <button
        type="button"
        className={arrow}
        onClick={() => next && onNavigate?.(next.id)}
        disabled={!next}
        aria-label="Next cluster"
      >
        <ChevronRight className="w-4 h-4" />
      </button>
    </div>
  );
}

/** Header summarising what the matcher concluded and how confidently. */
export function VerdictHeader({
  cluster,
  verdict,
  candidateCount,
  children,
}: {
  cluster: ClusterRecord;
  verdict: string;
  candidateCount: number;
  children?: ReactNode;
}) {
  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-3 flex-wrap">
        <div className="flex items-center gap-2">
          <span className="text-[9px] font-bold px-1.5 py-0.5 rounded-sm inline-block uppercase tracking-wider bg-amber-500/15 text-amber-700">
            Ambiguous Match
          </span>
          {cluster.created_at && (
            <span className="text-[10px] font-medium text-[#064E3B]/50">
              Flagged {formatRelativeDate(cluster.created_at).toLowerCase()}
            </span>
          )}
        </div>
        {children}
      </div>

      <p className="text-[15px] font-semibold text-[#064E3B]">{verdict}</p>
      {candidateCount > 1 && (
        <p className="text-[12px] text-[#064E3B]/60">
          {candidateCount} candidates found — pick the correct one below.
        </p>
      )}
      {candidateCount === 0 && (
        <p className="text-[12px] text-[#064E3B]/60">
          No existing transaction candidates were found for this evidence.
        </p>
      )}
    </div>
  );
}

/**
 * The four resolution verdicts.
 *
 * Each is presented with its consequence, since merging is destructive in a way
 * that is not obvious from a button label alone.
 */
export function ResolutionActions({
  actions,
  selectedMerchant,
}: {
  actions: Actions;
  selectedMerchant: string | undefined;
}) {
  const { isPending, candidates, selectedCandidateId } = actions;

  return (
    <div className="p-4 rounded-xl border border-[#064E3B]/10 flex flex-col gap-3 bg-[#F8E7C9]/50">
      <p className="text-[11px] font-medium flex items-center gap-1.5 text-[#064E3B]/60 uppercase tracking-wide">
        <SplitSquareHorizontal className="w-3.5 h-3.5" /> Resolution Actions
      </p>
      <p className="text-[11px] italic text-[#064E3B]/60">
        Review carefully — these actions alter your financial records.
      </p>

      <div className="grid grid-cols-2 gap-3">
        <ActionWithCaption caption="No changes made. Stays in your queue for another day.">
          <Button
            variant="ghost"
            className="w-full text-xs h-8 text-[#064E3B]/70 hover:bg-[#064E3B]/5 hover:text-[#064E3B]"
            onClick={actions.handleReviewLater}
            disabled={isPending}
          >
            <Clock className="w-3.5 h-3.5 mr-1.5" /> Later
          </Button>
        </ActionWithCaption>
        <ActionWithCaption caption="Marks this as not a duplicate — stays unmatched.">
          <Button
            variant="ghost"
            className="w-full text-xs h-8 text-red-600/80 hover:bg-red-50 hover:text-red-700"
            onClick={actions.handleRejectCandidates}
            disabled={isPending}
          >
            <Ban className="w-3.5 h-3.5 mr-1.5" /> Reject
          </Button>
        </ActionWithCaption>
      </div>

      <div className="flex flex-col gap-2">
        <ActionWithCaption caption="Creates a new transaction — both records stay independent.">
          <Button
            variant="outline"
            className="w-full text-[13px] h-9 border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5 hover:text-[#064E3B] font-semibold"
            onClick={actions.handleKeepSeparate}
            disabled={isPending}
          >
            Keep Separate
          </Button>
        </ActionWithCaption>
        <ActionWithCaption
          caption={
            selectedMerchant
              ? `Merges into "${selectedMerchant}" — stops appearing as a separate entry.`
              : 'Select a match above, then confirm.'
          }
        >
          <Button
            className="w-full text-[13px] h-9 font-semibold bg-[#064E3B] text-[#F8E7C9] hover:bg-[#064E3B]/90 focus-visible:ring-[#064E3B]"
            onClick={actions.handleConfirmMatch}
            disabled={isPending || candidates.length === 0 || !selectedCandidateId}
          >
            <GitMerge className="w-3.5 h-3.5 mr-1.5" /> Confirm Selected Match
          </Button>
        </ActionWithCaption>
      </div>
    </div>
  );
}
