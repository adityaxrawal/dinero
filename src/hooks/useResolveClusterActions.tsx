import { useState, useEffect } from 'react';
import { useResolveCluster } from '@/hooks/mutations/useResolveCluster';
import { useToast } from '@/hooks/use-toast';
import { getErrorToast } from '@/lib/errorMapping';
import { ToastAction } from '@/components/ui/toast';
import { API } from '@/lib/ipc';
import { confirmAction } from '@/lib/confirmDialog';
import type { ClusterRecord } from '@/lib/ipc';

interface UseResolveClusterActionsProps {
  cluster: ClusterRecord | undefined;
  onSuccess: () => void;
}

export function useResolveClusterActions({ cluster, onSuccess }: UseResolveClusterActionsProps) {
  const { toast } = useToast();
  const resolveCluster = useResolveCluster();

  const [selectedCandidateId, setSelectedCandidateId] = useState<string | null>(null);

  // Reset selection when cluster changes
  useEffect(() => {
    setSelectedCandidateId(null);
  }, [cluster?.id]);

  const incoming = cluster?.members.find((m) => m.member_role === 'incoming');
  const candidates = cluster?.members.filter((m) => m.member_role !== 'incoming') ?? [];
  const incomingObservationId = incoming?.observation_id;

  const confirmAndResolve = async (
    title: string,
    plainLanguageExplanation: string,
    action: 'confirm_match' | 'reject_candidate' | 'keep_separate' | 'mark_unresolved',
    chosenCanonicalId?: string
  ) => {
    if (!cluster?.id || !incomingObservationId) {
      toast({
        variant: 'destructive',
        title: 'Cannot resolve',
        description: 'This cluster has no incoming evidence to act on.',
      });
      return;
    }

    if (!(await confirmAction(plainLanguageExplanation, title))) return;

    const clusterIdSnapshot = cluster.id;

    resolveCluster.mutate(
      { clusterId: clusterIdSnapshot, observationId: incomingObservationId, action, chosenCanonicalId },
      {
        onSuccess: () => {
          if (action === 'mark_unresolved') {
            toast({
              title: 'Marked for Later Review',
              description: 'This cluster will stay in your queue.',
            });
          } else if (action === 'confirm_match') {
            toast({
              title: 'Cluster Resolved',
              description: 'Your decision has been recorded.',
              action: (
                <ToastAction
                  altText="Undo"
                  onClick={() => {
                    API.reconciliation.unmergeCluster(clusterIdSnapshot).catch((err) => {
                      toast({ variant: 'destructive', ...getErrorToast(err) });
                    });
                  }}
                >
                  Undo
                </ToastAction>
              ),
            });
          } else {
            toast({ title: 'Cluster Resolved', description: 'Your decision has been recorded.' });
          }
          onSuccess();
        },
        onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }),
      }
    );
  };

  const handleConfirmMatch = () => {
    const candidate = candidates.find((c) => c.canonical_transaction_id === selectedCandidateId);
    if (!candidate) {
      toast({
        variant: 'destructive',
        title: 'Pick a match first',
        description: 'Select which existing transaction this matches before confirming.',
      });
      return;
    }
    confirmAndResolve(
      'Confirm Match',
      `Link this transaction to "${candidate.merchant}" (₹${Math.abs(candidate.amount).toFixed(2)})? The two records will be merged as the same transaction.`,
      'confirm_match',
      selectedCandidateId ?? undefined
    );
  };

  const handleRejectCandidates = () => {
    confirmAndResolve(
      'Reject Matches',
      'None of the existing transactions shown are a match. The new evidence will be recorded as a separate, unmatched transaction.',
      'reject_candidate'
    );
  };

  const handleKeepSeparate = () => {
    confirmAndResolve(
      'Keep Separate',
      'This transaction is genuinely distinct from the ones shown, even though they look similar. It will be kept as its own separate transaction.',
      'keep_separate'
    );
  };

  const handleReviewLater = () => {
    confirmAndResolve(
      'Review Later',
      "This cluster will stay in your queue and won't be resolved yet.",
      'mark_unresolved'
    );
  };

  return {
    candidates,
    selectedCandidateId,
    setSelectedCandidateId,
    handleConfirmMatch,
    handleRejectCandidates,
    handleKeepSeparate,
    handleReviewLater,
    isPending: resolveCluster.isPending,
  };
}
