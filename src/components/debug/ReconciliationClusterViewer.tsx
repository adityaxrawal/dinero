/**
 * Raw view of reconciliation clusters and their members.
 */
import { useEffect, useState } from 'react';
import { API } from '../../lib/ipc';
import { Badge } from '../ui/badge';

import { DebugTableLayout } from './DebugTableLayout';

interface ReconciliationCluster {
  id: string;
  created_at: string;
  status: string;
  total_amount_minor: number | null;
  currency: string;
  observation_id: string | null;
}

/** Raw view of reconciliation clusters. */
export function ReconciliationClusterViewer() {
  const [clusters, setClusters] = useState<ReconciliationCluster[]>([]);
  const [loading, setLoading] = useState(true);

  /** Loads clusters in raw form. */
  const fetchClusters = async () => {
    setLoading(true);
    try {
      const data = await API.debug.fetchReconciliationClusters();
      setClusters(data);
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchClusters();
  }, []);

  return (
    <DebugTableLayout
      title="Reconciliation Clusters"
      onRefresh={fetchClusters}
      loading={loading}
      data={clusters}
      loadingMessage="Loading clusters..."
      emptyMessage="No clusters found."
      headers={
        <>
          <th className="p-2 text-sm font-medium text-muted-foreground">ID</th>
          <th className="p-2 text-sm font-medium text-muted-foreground">Created At</th>
          <th className="p-2 text-sm font-medium text-muted-foreground">Status</th>
          <th className="p-2 text-sm font-medium text-muted-foreground">Amount</th>
          <th className="p-2 text-sm font-medium text-muted-foreground">Observation ID</th>
        </>
      }
      renderRow={(cluster) => (
        <tr key={cluster.id} className="border-b border-[var(--border-color)] last:border-0">
          <td className="p-2 text-sm font-mono">{cluster.id.substring(0, 8)}</td>
          <td className="p-2 text-sm">{new Date(cluster.created_at).toLocaleString()}</td>
          <td className="p-2 text-sm">
            {cluster.status === 'resolved' ? (
              <Badge variant="outline" className="bg-green-500/10 text-green-700">
                Resolved
              </Badge>
            ) : (
              <Badge variant="outline" className="bg-yellow-500/10 text-yellow-500">
                {cluster.status}
              </Badge>
            )}
          </td>
          <td className="p-2 text-sm font-mono">
            {cluster.total_amount_minor != null
              ? (cluster.total_amount_minor / 100).toFixed(2)
              : '-'}{' '}
            {cluster.currency}
          </td>
          <td className="p-2 text-sm font-mono text-muted-foreground">
            {cluster.observation_id?.substring(0, 8)}
          </td>
        </tr>
      )}
    />
  );
}
