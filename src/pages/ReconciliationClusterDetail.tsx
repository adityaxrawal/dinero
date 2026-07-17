import { useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { ArrowLeft, Loader2, GitMerge, X, SplitSquareHorizontal, Clock } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card';
import { useToast } from '@/hooks/use-toast';
import { getErrorMessage } from '@/lib/getErrorMessage';
import { useReconciliationCluster } from '@/hooks/queries/useReconciliationCluster';
import { useResolveCluster } from '@/hooks/mutations/useResolveCluster';
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
  const { clusterId } = useParams<{ clusterId: string }>();
  const navigate = useNavigate();
  const { toast } = useToast();
  const { data: cluster, isLoading, isError } = useReconciliationCluster(clusterId);
  const resolveCluster = useResolveCluster();

  const [selectedCandidateId, setSelectedCandidateId] = useState<string | null>(null);

  if (!clusterId) return null;

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-40" role="status" aria-label="Loading cluster">
        <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" aria-hidden="true" />
      </div>
    );
  }

  if (isError || !cluster) {
    return (
      <div className="max-w-3xl mx-auto space-y-4">
        <Button variant="ghost" size="sm" onClick={() => navigate('/reconciliation')}>
          <ArrowLeft className="w-4 h-4 mr-1" aria-hidden="true" /> Back
        </Button>
        <p className="text-sm text-muted-foreground">This cluster could not be found. It may already be resolved.</p>
      </div>
    );
  }

  const incoming = cluster.members.find((m) => m.member_role === 'incoming');
  const candidates = cluster.members.filter((m) => m.member_role !== 'incoming');

  const confirmAndResolve = async (
    title: string,
    plainLanguageExplanation: string,
    action: 'confirm_match' | 'reject_candidate' | 'keep_separate' | 'mark_unresolved',
    chosenCanonicalId?: string,
  ) => {
    if (!incoming?.observation_id) {
      toast({ variant: 'destructive', title: 'Cannot resolve', description: 'This cluster has no incoming evidence to act on.' });
      return;
    }

    let confirmed: boolean;
    try {
      const { ask } = await import('@tauri-apps/plugin-dialog');
      confirmed = await ask(plainLanguageExplanation, { title, kind: 'warning' });
    } catch {
      confirmed = confirm(plainLanguageExplanation);
    }
    if (!confirmed) return;

    resolveCluster.mutate(
      { clusterId, observationId: incoming.observation_id, action, chosenCanonicalId },
      {
        onSuccess: () => {
          toast(
            action === 'mark_unresolved'
              ? { title: 'Marked for Later Review', description: 'This cluster will stay in your queue.' }
              : { title: 'Cluster Resolved', description: 'Your decision has been recorded.' },
          );
          navigate('/reconciliation');
        },
        onError: (err) =>
          toast({ variant: 'destructive', title: 'Resolution Failed', description: getErrorMessage(err) }),
      },
    );
  };

  const handleConfirmMatch = () => {
    const candidate = candidates.find((c) => c.canonical_transaction_id === selectedCandidateId);
    if (!candidate) {
      toast({ variant: 'destructive', title: 'Pick a match first', description: 'Select which existing transaction this matches before confirming.' });
      return;
    }
    confirmAndResolve(
      'Confirm Match',
      `Link this transaction to "${candidate.merchant}" (₹${Math.abs(candidate.amount).toFixed(2)})? The two records will be merged as the same transaction.`,
      'confirm_match',
      selectedCandidateId ?? undefined,
    );
  };

  const handleRejectCandidates = () => {
    confirmAndResolve(
      'Reject Matches',
      'None of the existing transactions shown are a match. The new evidence will be recorded as a separate, unmatched transaction.',
      'reject_candidate',
    );
  };

  const handleKeepSeparate = () => {
    confirmAndResolve(
      'Keep Separate',
      'This transaction is genuinely distinct from the ones shown, even though they look similar. It will be kept as its own separate transaction.',
      'keep_separate',
    );
  };

  const handleReviewLater = () => {
    confirmAndResolve(
      'Review Later',
      "This cluster will stay in your queue and won't be resolved yet.",
      'mark_unresolved',
    );
  };

  const isPending = resolveCluster.isPending;

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
