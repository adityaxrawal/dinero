
import { useParams, useNavigate } from 'react-router-dom';
import { ArrowLeft, Loader2, GitMerge, X, SplitSquareHorizontal, Clock } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card';

import { useReconciliationCluster } from '@/hooks/queries/useReconciliationCluster';
import { useResolveClusterActions } from '@/hooks/useResolveClusterActions';
import ClusterMemberComparison from '@/components/reconciliation/ClusterMemberComparison';
import type { ClusterMember } from '@/lib/ipc';

/**
 * TASK-FE-013 (Doc 30): "Action buttons map to the three resolution
 * actions, each with a plain-language explanation before confirming (these
 * are financial-record-altering decisions)." Reuses this codebase's
 * established confirm-dialog pattern (`@tauri-apps/plugin-dialog`'s `ask()`,
 * first wired for delete flows in TASK-FE-011) rather than a bespoke modal.
 *
 * Every resolution call requires the real `observation_id` of the
 * cluster's "incoming" member (Document 12's `resolve_cluster`, invoked by
 * `reconciliation_clusters_resolve`) -- not any member row's own `id`. The
 * pre-rewrite page never sent one at all.
 */
export default function ReconciliationClusterDetail() {

  const navigate = useNavigate();
  const { clusterId } = useParams<{ clusterId: string }>();

  const { data: cluster, isLoading, error } = useReconciliationCluster(clusterId);

  const {
    candidates,
    selectedCandidateId,
    setSelectedCandidateId,
    handleConfirmMatch,
    handleRejectCandidates,
    handleKeepSeparate,
    handleReviewLater,
    isPending,
  } = useResolveClusterActions({
    cluster,
    onSuccess: () => {
      navigate('/reconciliation');
    },
  });

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-[50vh]">
        <Loader2 className="w-8 h-8 animate-spin text-muted-foreground" aria-hidden="true" />
      </div>
    );
  }

  if (error || !cluster) {
    return (
      <div className="flex flex-col items-center justify-center h-[50vh] space-y-4">
        <p className="text-xl font-semibold">Cluster Not Found</p>
        <p className="text-sm text-muted-foreground">This cluster could not be found. It may already be resolved.</p>
      </div>
    );
  }

  return (
    <div className="space-y-6 max-w-4xl mx-auto">
      <Button variant="ghost" size="sm" onClick={() => navigate('/reconciliation')}>
        <ArrowLeft className="w-4 h-4 mr-1" aria-hidden="true" /> Back
      </Button>

      <div>
        <h1 className="text-2xl font-bold">Ambiguous Match Cluster</h1>
        <p className="text-muted-foreground mt-1">
          {cluster.reason.startsWith('Ambiguous match') ? (
            <><span>Ambiguous match</span>{cluster.reason.substring(15)}</>
          ) : cluster.reason}
        </p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Compare Evidence</CardTitle>
          <CardDescription>
            {candidates.length > 0
              ? 'Click an existing match to select it before confirming.'
              : 'No existing transaction candidates were found for this evidence.'}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <ClusterMemberComparison
            members={cluster.members}
            selectedCandidateId={selectedCandidateId}
            onSelectCandidate={(m: ClusterMember) => setSelectedCandidateId(m.canonical_transaction_id)}
          />
        </CardContent>
      </Card>

      <Card className="bg-muted/30">
        <CardContent className="p-4 flex flex-col sm:flex-row justify-between items-start sm:items-center gap-4">
          <p className="text-sm text-muted-foreground italic flex items-center gap-2">
            <SplitSquareHorizontal className="w-4 h-4" aria-hidden="true" /> Review carefully — these actions alter your financial records.
          </p>
          <div className="flex flex-wrap gap-2">
            <Button variant="ghost" className="text-muted-foreground" onClick={handleReviewLater} disabled={isPending}>
              <Clock className="w-4 h-4 mr-2" aria-hidden="true" /> Review Later
            </Button>
            <Button variant="outline" className="text-red-700 hover:text-red-700 hover:bg-destructive/10" onClick={handleRejectCandidates} disabled={isPending}>
              <X className="w-4 h-4 mr-2" aria-hidden="true" /> Reject Matches
            </Button>
            <Button variant="outline" onClick={handleKeepSeparate} disabled={isPending}>
              Keep Separate
            </Button>
            <Button
              onClick={handleConfirmMatch}
              disabled={isPending || candidates.length === 0 || !selectedCandidateId}
              variant="outline"
              className="border-emerald-500/50 text-emerald-700 hover:bg-emerald-500/10 hover:text-emerald-800"
            >
              <GitMerge className="w-4 h-4 mr-2" aria-hidden="true" /> Confirm Match
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
