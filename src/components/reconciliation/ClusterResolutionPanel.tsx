/**
 * Actions for resolving a cluster: confirm, reject, keep separate, defer.
 */
import { useReconciliationCluster } from '@/hooks/queries/useReconciliationCluster';
import { useResolveClusterActions } from '@/hooks/useResolveClusterActions';
import { summarizeClusterDiff } from '@/lib/clusterDiff';
import ClusterMemberComparison from '@/components/reconciliation/ClusterMemberComparison';
import type { ClusterRecord } from '@/lib/ipc';
import {
  ClusterLoading,
  ClusterNotFound,
  QueueNav,
  ResolutionActions,
  VerdictHeader,
} from './cluster/ClusterPanelParts';

interface ClusterResolutionPanelProps {
  clusterId: string | undefined;
  initialCluster?: ClusterRecord;
  onResolved: () => void;
  queueClusters?: ClusterRecord[] | undefined;
  onNavigate?: ((clusterId: string) => void) | undefined;
}

/** The four resolution verdicts, each with its consequence stated. */
export default function ClusterResolutionPanel({
  clusterId,
  initialCluster,
  onResolved,
  queueClusters,
  onNavigate,
}: ClusterResolutionPanelProps) {
  const { data: detailCluster, isLoading } = useReconciliationCluster(clusterId);
  const cluster = detailCluster ?? initialCluster;
  const actions = useResolveClusterActions({ cluster, onSuccess: onResolved });

  if (!cluster) return isLoading ? <ClusterLoading /> : <ClusterNotFound />;

  const { candidates, selectedCandidateId } = actions;
  const reference = cluster.members.find((m) => m.member_role === 'incoming');
  const verdict = summarizeClusterDiff(reference, candidates) ?? cluster.explanation;
  const selectedMerchant = candidates.find(
    (c) => c.canonical_transaction_id === selectedCandidateId
  )?.merchant;

  return (
    <div className="space-y-6">
      <VerdictHeader cluster={cluster} verdict={verdict} candidateCount={candidates.length}>
        {queueClusters && (
          <QueueNav
            queueClusters={queueClusters}
            clusterId={clusterId}
            onNavigate={onNavigate}
          />
        )}
      </VerdictHeader>

      <ClusterMemberComparison
        members={cluster.members}
        selectedCandidateId={selectedCandidateId}
        onSelectCandidate={(m) => actions.setSelectedCandidateId(m.canonical_transaction_id)}
      />

      <ResolutionActions actions={actions} selectedMerchant={selectedMerchant} />
    </div>
  );
}
