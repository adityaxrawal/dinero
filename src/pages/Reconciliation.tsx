import { useNavigate } from 'react-router-dom';
import { Check, ShieldAlert, HelpCircle, ChevronRight, Loader2 } from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { useReconciliationClusters } from '@/hooks/queries/useReconciliationClusters';
import { useUnassignedTransactions } from '@/hooks/queries/useUnassignedTransactions';

/**
 * TASK-FE-013 (Doc 30): "Two clearly-labeled sections: pending clusters and
 * unassigned transactions." Ambiguous-match clusters (an instrument *was*
 * resolved, but which existing transaction it matches is unclear) and
 * unassigned transactions (no instrument could be resolved at all --
 * TASK-API-005's `reconciliation_get_unassigned_transactions`, which had no
 * frontend call site before this task) are structurally separate queues per
 * the backend, so they stay visually separate here rather than merged into
 * one undifferentiated list.
 *
 * Each cluster only shows a compact preview; the full side-by-side
 * comparison and the three resolution actions live on
 * `ReconciliationClusterDetail`, reached via "Review Cluster".
 */
export default function Reconciliation() {
  const navigate = useNavigate();
  const { data: clusters = [], isLoading: clustersLoading } = useReconciliationClusters();
  const { data: unassigned = [], isLoading: unassignedLoading } = useUnassignedTransactions();

  const isLoading = clustersLoading || unassignedLoading;
  const allCaughtUp = !isLoading && clusters.length === 0 && unassigned.length === 0;

  return (
    <div className="space-y-8 max-w-4xl mx-auto">
      <header>
        <h1 className="text-3xl font-bold tracking-tight">Reconciliation</h1>
        <p className="text-muted-foreground mt-1">Resolve ambiguous and unassigned transactions</p>
      </header>

      {isLoading ? (
        <div className="flex justify-center items-center h-40 text-muted-foreground" role="status" aria-label="Loading reconciliation queue">
          <Loader2 className="w-5 h-5 animate-spin" aria-hidden="true" />
        </div>
      ) : allCaughtUp ? (
        <Card className="border-dashed bg-secondary/20">
          <CardContent className="flex flex-col items-center justify-center py-16 text-center">
            <div className="w-16 h-16 rounded-full bg-emerald-500/10 flex items-center justify-center mb-4">
              <Check className="w-8 h-8 text-emerald-700" />
            </div>
            <h3 className="text-lg font-semibold mb-1">All Caught Up!</h3>
            <p className="text-sm text-muted-foreground">
              No pending clusters or unassigned transactions require manual review.
            </p>
          </CardContent>
        </Card>
      ) : (
        <div className="space-y-10">
          <section aria-label="Pending Clusters">
            <h2 className="text-lg font-semibold mb-3 flex items-center gap-2">
              <ShieldAlert className="w-4 h-4 text-amber-500" aria-hidden="true" />
              Pending Clusters
              {clusters.length > 0 && <Badge variant="outline">{clusters.length}</Badge>}
            </h2>
            {clusters.length === 0 ? (
              <p className="text-sm text-muted-foreground">No ambiguous matches waiting on review.</p>
            ) : (
              <div className="space-y-3">
                {clusters.map((cluster) => (
                  <Card key={cluster.id} className="border-border/60">
                    <CardContent className="p-4 flex items-center justify-between gap-4">
                      <div>
                        <p className="font-medium text-sm">
                          {cluster.reason.startsWith('Ambiguous match') ? (
                            <><span>Ambiguous match</span>{cluster.reason.substring(15)}</>
                          ) : cluster.reason}
                        </p>
                        <p className="text-xs text-muted-foreground mt-0.5">
                          {cluster.members_count} member{cluster.members_count === 1 ? '' : 's'} · ID: {cluster.id}
                        </p>
                      </div>
                      <Button size="sm" onClick={() => navigate(`/reconciliation/${cluster.id}`)}>
                        Review Cluster <ChevronRight className="w-4 h-4 ml-1" aria-hidden="true" />
                      </Button>
                    </CardContent>
                  </Card>
                ))}
              </div>
            )}
          </section>

          <section aria-label="Unassigned Transactions">
            <h2 className="text-lg font-semibold mb-3 flex items-center gap-2">
              <HelpCircle className="w-4 h-4 text-amber-500" aria-hidden="true" />
              Unassigned Transactions
              {unassigned.length > 0 && <Badge variant="outline">{unassigned.length}</Badge>}
            </h2>
            {unassigned.length === 0 ? (
              <p className="text-sm text-muted-foreground">No transactions failed instrument resolution.</p>
            ) : (
              <div className="space-y-2">
                {unassigned.map((item) => (
                  <Card key={item.id} className="border-border/60">
                    <CardHeader className="p-4 pb-0">
                      <CardTitle className="text-sm font-medium">{item.reason || 'Unresolved'}</CardTitle>
                    </CardHeader>
                    <CardContent className="p-4 pt-2 text-xs text-muted-foreground">
                      Observation ID: {item.observation_id}
                      {item.created_at && <> · {item.created_at}</>}
                    </CardContent>
                  </Card>
                ))}
              </div>
            )}
          </section>
        </div>
      )}
    </div>
  );
}
