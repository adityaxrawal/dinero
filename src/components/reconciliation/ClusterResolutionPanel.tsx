import type { ReactNode } from 'react';
import { Loader2, GitMerge, Ban, Clock, SplitSquareHorizontal, ChevronLeft, ChevronRight } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { formatRelativeDate } from '@/lib/utils';
import { useReconciliationCluster } from '@/hooks/queries/useReconciliationCluster';
import { useResolveClusterActions } from '@/hooks/useResolveClusterActions';
import { summarizeClusterDiff } from '@/lib/clusterDiff';
import ClusterMemberComparison from '@/components/reconciliation/ClusterMemberComparison';
import type { ClusterRecord } from '@/lib/ipc';

interface ClusterResolutionPanelProps {
  clusterId: string | undefined;
  /** Lighter row from the list query, rendered instantly while the full detail fetch is in flight. */
  initialCluster?: ClusterRecord;
  onResolved: () => void;
  /** Full unresolved-cluster list, only needed to show "N of M" + prev/next queue navigation. */
  queueClusters?: ClusterRecord[] | undefined;
  onNavigate?: ((clusterId: string) => void) | undefined;
}

function ActionWithCaption({ caption, children }: { caption: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      {children}
      <p className="text-[10px] text-center text-muted-foreground leading-snug px-1">{caption}</p>
    </div>
  );
}

export default function ClusterResolutionPanel({
  clusterId,
  initialCluster,
  onResolved,
  queueClusters,
  onNavigate,
}: ClusterResolutionPanelProps) {
  const { data: detailCluster, isLoading } = useReconciliationCluster(clusterId);
  const cluster = detailCluster ?? initialCluster;

  const {
    candidates,
    selectedCandidateId,
    setSelectedCandidateId,
    handleConfirmMatch,
    handleRejectCandidates,
    handleKeepSeparate,
    handleReviewLater,
    isPending,
  } = useResolveClusterActions({ cluster, onSuccess: onResolved });

  if (!cluster) {
    if (isLoading) {
      return (
        <div className="flex items-center justify-center py-16 gap-2 text-sm text-muted-foreground">
          <Loader2 className="w-4 h-4 animate-spin" aria-hidden="true" /> Loading cluster details…
        </div>
      );
    }
    return (
      <div className="flex flex-col items-center justify-center py-16 gap-2 text-center">
        <p className="text-base font-semibold text-[#064E3B]">Cluster Not Found</p>
        <p className="text-sm text-muted-foreground">
          This cluster could not be found. It may already be resolved.
        </p>
      </div>
    );
  }

  const reference = cluster.members.find((m) => m.member_role === 'incoming');
  const verdict = summarizeClusterDiff(reference, candidates) ?? cluster.explanation;
  const selectedCandidate = candidates.find((c) => c.canonical_transaction_id === selectedCandidateId);

  const queueIndex = queueClusters?.findIndex((c) => c.id === clusterId) ?? -1;
  const hasQueueNav = !!queueClusters && queueIndex >= 0;
  const prevCluster = hasQueueNav && queueIndex > 0 ? queueClusters![queueIndex - 1] : null;
  const nextCluster =
    hasQueueNav && queueIndex < queueClusters!.length - 1 ? queueClusters![queueIndex + 1] : null;

  return (
    <div className="space-y-6">
      {/* Verdict header */}
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

          {hasQueueNav && (
            <div className="flex items-center gap-1 text-[11px] font-medium text-[#064E3B]/60">
              <button
                type="button"
                className="w-6 h-6 flex items-center justify-center rounded-md hover:bg-[#064E3B]/10 disabled:opacity-30 disabled:hover:bg-transparent"
                onClick={() => prevCluster && onNavigate?.(prevCluster.id)}
                disabled={!prevCluster}
                aria-label="Previous cluster"
              >
                <ChevronLeft className="w-4 h-4" />
              </button>
              <span>
                {queueIndex + 1} of {queueClusters!.length}
              </span>
              <button
                type="button"
                className="w-6 h-6 flex items-center justify-center rounded-md hover:bg-[#064E3B]/10 disabled:opacity-30 disabled:hover:bg-transparent"
                onClick={() => nextCluster && onNavigate?.(nextCluster.id)}
                disabled={!nextCluster}
                aria-label="Next cluster"
              >
                <ChevronRight className="w-4 h-4" />
              </button>
            </div>
          )}
        </div>

        <p className="text-[15px] font-semibold text-[#064E3B]">{verdict}</p>
        {candidates.length > 1 && (
          <p className="text-[12px] text-[#064E3B]/60">
            {candidates.length} candidates found — pick the correct one below.
          </p>
        )}
        {candidates.length === 0 && (
          <p className="text-[12px] text-[#064E3B]/60">
            No existing transaction candidates were found for this evidence.
          </p>
        )}
      </div>

      {/* Evidence comparison */}
      <ClusterMemberComparison
        members={cluster.members}
        selectedCandidateId={selectedCandidateId}
        onSelectCandidate={(m) => setSelectedCandidateId(m.canonical_transaction_id)}
      />

      {/* Action Zone */}
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
              onClick={handleReviewLater}
              disabled={isPending}
            >
              <Clock className="w-3.5 h-3.5 mr-1.5" /> Later
            </Button>
          </ActionWithCaption>
          <ActionWithCaption caption="Marks this as not a duplicate — stays unmatched.">
            <Button
              variant="ghost"
              className="w-full text-xs h-8 text-red-600/80 hover:bg-red-50 hover:text-red-700"
              onClick={handleRejectCandidates}
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
              onClick={handleKeepSeparate}
              disabled={isPending}
            >
              Keep Separate
            </Button>
          </ActionWithCaption>
          <ActionWithCaption
            caption={
              selectedCandidate
                ? `Merges into "${selectedCandidate.merchant}" — stops appearing as a separate entry.`
                : 'Select a match above, then confirm.'
            }
          >
            <Button
              className="w-full text-[13px] h-9 font-semibold bg-[#064E3B] text-[#F8E7C9] hover:bg-[#064E3B]/90 focus-visible:ring-[#064E3B]"
              onClick={handleConfirmMatch}
              disabled={isPending || candidates.length === 0 || !selectedCandidateId}
            >
              <GitMerge className="w-3.5 h-3.5 mr-1.5" /> Confirm Selected Match
            </Button>
          </ActionWithCaption>
        </div>
      </div>
    </div>
  );
}
