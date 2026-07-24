import { X, GitMerge, SplitSquareHorizontal, Clock, Ban } from 'lucide-react';
import { useQueryClient } from '@tanstack/react-query';
import { cn } from '@/lib/utils';
import { useReconciliationCluster } from '@/hooks/queries/useReconciliationCluster';
import { useResolveClusterActions } from '@/hooks/useResolveClusterActions';
import ClusterMemberComparison from '@/components/reconciliation/ClusterMemberComparison';
import { queryKeys } from '@/lib/queryKeys';
import type { ClusterRecord } from '@/lib/ipc';
import { Button } from '@/components/ui/button';

interface ReconciliationInspectorProps {
  cluster: ClusterRecord | undefined;
  onClose: () => void;
  inline?: boolean;
}

export default function ReconciliationInspector({
  cluster: initialCluster,
  onClose,
  inline = false,
}: ReconciliationInspectorProps) {
  const queryClient = useQueryClient();
  const isOpen = !!initialCluster;

  const { data: detailCluster, isLoading } = useReconciliationCluster(initialCluster?.id);

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
  } = useResolveClusterActions({
    cluster,
    onSuccess: () => {
      onClose();
      queryClient.invalidateQueries({ queryKey: queryKeys.reconciliation.unresolved() });
    },
  });

  const asideClasses = cn(
    !inline && 'inspector-panel',
    !inline && (!cluster || !isOpen) && 'closed',
    inline && 'w-full h-full flex flex-col',
    !inline && 'flex-shrink-0'
  );

  const asideStyle = inline
    ? { backgroundColor: '#F8E7C9' }
    : { width: cluster && isOpen ? 'var(--inspector-width)' : 0 };

  if (!cluster) {
    return (
      <aside className={asideClasses} role="complementary" aria-hidden={true} style={asideStyle} />
    );
  }

  return (
    <aside
      className={asideClasses}
      role="complementary"
      aria-label="Cluster detail"
      aria-hidden={!isOpen}
      style={asideStyle}
    >
      {/* Header */}
      <div
        className={cn('flex items-start justify-between p-5 flex-shrink-0', inline && 'pt-0')}
        style={{ borderBottom: '1px solid rgba(6,78,59,0.1)' }}
      >
        <div className="min-w-0 flex-1 pr-3">
          <span
            className="text-[9px] font-bold px-1.5 py-0.5 rounded-sm mb-2 inline-block uppercase tracking-wider"
            style={{ background: 'rgba(217,119,6,0.15)', color: '#d97706' }}
          >
            Ambiguous Match
          </span>
          <p className="text-[15px] font-semibold text-[#064E3B]">
            {cluster.reason.startsWith('Ambiguous match')
              ? cluster.reason.substring(15).trim()
              : cluster.reason}
          </p>
        </div>

        <button
          type="button"
          className="w-8 h-8 flex items-center justify-center rounded-lg transition-colors hover:bg-[#064E3B]/10 text-[#064E3B]/60 hover:text-[#064E3B] flex-shrink-0"
          onClick={onClose}
          aria-label="Close inspector"
        >
          <X className="w-5 h-5" />
        </button>
      </div>

      {/* Content */}
      <div
        className={cn(
          'flex-1 overflow-y-auto flex flex-col',
          inline ? 'p-8 max-w-3xl mx-auto w-full' : 'p-4'
        )}
      >
        <div className="flex-1">
          <div className="mb-4">
            <h3 className="text-sm font-semibold mb-1 text-[#064E3B]">Compare Evidence</h3>
            <p className="text-xs text-[#064E3B]/70">
              {candidates.length > 0
                ? 'Click an existing match to select it before confirming.'
                : 'No existing transaction candidates were found for this evidence.'}
            </p>
          </div>

          {isLoading && !detailCluster ? (
            <div
              className="flex items-center justify-center py-12 text-sm gap-2"
              style={{ color: '#6b8a7f' }}
            >
              Loading details…
            </div>
          ) : (
            <ClusterMemberComparison
              members={cluster.members}
              selectedCandidateId={selectedCandidateId}
              onSelectCandidate={(m) => setSelectedCandidateId(m.canonical_transaction_id)}
            />
          )}
        </div>

        {/* Action Zone at bottom */}
        <div className="mt-6 p-4 rounded-xl border border-[#064E3B]/10 flex flex-col gap-3 bg-[#F8E7C9]/50">
          <p className="text-[11px] font-medium flex items-center gap-1.5 text-[#064E3B]/60 uppercase tracking-wide">
            <SplitSquareHorizontal className="w-3.5 h-3.5" /> Resolution Actions
          </p>

          <div className="grid grid-cols-2 gap-2">
            <Button
              variant="outline"
              className="w-full text-xs h-8 border-[#064E3B]/10 text-[#064E3B] hover:bg-[#064E3B]/5 hover:text-[#064E3B]"
              onClick={handleReviewLater}
              disabled={isPending}
            >
              <Clock className="w-3.5 h-3.5 mr-1.5" /> Later
            </Button>
            <Button
              variant="outline"
              className="w-full text-xs h-8 border-red-200 text-red-600 hover:bg-red-50 hover:text-red-700 hover:border-red-300"
              onClick={handleRejectCandidates}
              disabled={isPending}
            >
              <Ban className="w-3.5 h-3.5 mr-1.5" /> Reject
            </Button>
            <Button
              variant="outline"
              className="w-full text-[13px] h-8 col-span-2 border-[#064E3B]/10 text-[#064E3B] hover:bg-[#064E3B]/5 hover:text-[#064E3B] font-semibold"
              onClick={handleKeepSeparate}
              disabled={isPending}
            >
              Keep Separate
            </Button>
            <Button
              className="w-full text-[13px] h-8 col-span-2 font-semibold bg-[#064E3B] text-[#F8E7C9] hover:bg-[#064E3B]/90 focus-visible:ring-[#064E3B]"
              onClick={handleConfirmMatch}
              disabled={isPending || candidates.length === 0 || !selectedCandidateId}
            >
              <GitMerge className="w-3.5 h-3.5 mr-1.5" /> Confirm Selected Match
            </Button>
          </div>
        </div>
      </div>
    </aside>
  );
}
