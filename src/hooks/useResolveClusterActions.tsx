import { useState, useEffect } from 'react';
import { useResolveCluster } from '@/hooks/mutations/useResolveCluster';
import { useToast } from '@/hooks/use-toast';
import { getErrorToast } from '@/lib/errorMapping';
import { ToastAction } from '@/components/ui/toast';
import { API } from '@/lib/ipc';
import { confirmAction } from '@/lib/confirmDialog';
import type { ClusterRecord } from '@/lib/ipc';

/**
 * The interaction layer around resolving a reconciliation cluster.
 *
 * Sits between the cluster UI and the raw resolve mutation, adding the three
 * things a merge decision needs beyond the API call itself: an explicit
 * confirmation step, outcome-specific feedback, and an undo affordance.
 *
 * Confirmation matters because merging transactions is destructive in a way
 * users cannot easily reverse by hand -- two records become one. The confirm
 * copy is passed in by the caller as plain language rather than being generated
 * here, so each action can explain its own consequences precisely.
 *
 * A JSX file rather than .ts because the undo toast embeds a React element.
 */
interface UseResolveClusterActionsProps {
  cluster: ClusterRecord | undefined;
  onSuccess: () => void;
}

/** Confirmation, feedback and undo around resolving a cluster. */
export function useResolveClusterActions({ cluster, onSuccess }: UseResolveClusterActionsProps) {
  const { toast } = useToast();
  const resolveCluster = useResolveCluster();

  const [selectedCandidateId, setSelectedCandidateId] = useState<string | null>(null);

  // Clear the selection when the cluster changes, so a candidate chosen in the
  // previous cluster cannot carry over and be acted on in the next one.
  useEffect(() => {
    setSelectedCandidateId(null);
  }, [cluster?.id]);

  // A cluster is one incoming observation weighed against existing candidates.
  // The incoming member is the thing being resolved; everything else is what it
  // might match.
  const incoming = cluster?.members.find((m) => m.member_role === 'incoming');
  const candidates = cluster?.members.filter((m) => m.member_role !== 'incoming') ?? [];
  const incomingObservationId = incoming?.observation_id;

  /**
   * Confirm with the user, then submit the resolution.
   *
   * Shared by every action the UI offers; the differences between them are
   * carried entirely in the arguments.
   */
  const confirmAndResolve = async (
    title: string,
    plainLanguageExplanation: string,
    action: 'confirm_match' | 'reject_candidate' | 'keep_separate' | 'mark_unresolved',
    chosenCanonicalId?: string
  ) => {
    // Without an incoming observation there is nothing to resolve. Reported as
    // a toast rather than silently ignored, so a malformed cluster is visible
    // instead of presenting buttons that quietly do nothing.
    if (!cluster?.id || !incomingObservationId) {
      toast({
        variant: 'destructive',
        title: 'Cannot resolve',
        description: 'This cluster has no incoming evidence to act on.',
      });
      return;
    }

    if (!(await confirmAction(plainLanguageExplanation, title))) return;

    // Captured before the mutation runs. The undo handler below closes over
    // this, and by the time undo is clicked the cluster prop may have changed
    // or gone -- reading it from the closure would target the wrong cluster.
    const clusterIdSnapshot = cluster.id;

    resolveCluster.mutate(
      { clusterId: clusterIdSnapshot, observationId: incomingObservationId, action, chosenCanonicalId },
      {
        // Feedback varies by outcome. Only a confirmed match gets an undo
        // affordance -- it is the one action that actually merged records and
        // therefore the one with something to reverse. Deferring or keeping
        // entries separate leaves the data untouched.
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

  /**
   * Merge the incoming observation into the selected existing transaction.
   *
   * Requires a selection: this is the one action that needs a target, so the
   * guard reports the omission rather than silently merging into nothing. The
   * confirmation names the merchant and amount, so the user is agreeing to a
   * specific merge rather than an abstract one.
   */
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

  /** None of the candidates match; record the evidence as its own transaction. */
  const handleRejectCandidates = () => {
    confirmAndResolve(
      'Reject Matches',
      'None of the existing transactions shown are a match. The new evidence will be recorded as a separate, unmatched transaction.',
      'reject_candidate'
    );
  };

  /**
   * These are genuinely distinct despite looking alike.
   *
   * Differs from rejecting candidates in intent: this asserts the similarity was
   * coincidental, which is a signal the matcher can learn from.
   */
  const handleKeepSeparate = () => {
    confirmAndResolve(
      'Keep Separate',
      'This transaction is genuinely distinct from the ones shown, even though they look similar. It will be kept as its own separate transaction.',
      'keep_separate'
    );
  };

  /** Defer the decision, leaving the cluster in the queue untouched. */
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
