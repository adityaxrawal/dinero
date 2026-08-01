import { useParams, useNavigate } from 'react-router-dom';
import { ArrowLeft } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useReconciliationClusters } from '@/hooks/queries/useReconciliationClusters';
import ClusterResolutionPanel from '@/components/reconciliation/ClusterResolutionPanel';

export default function ReconciliationClusterDetail() {
  const navigate = useNavigate();
  const { clusterId } = useParams<{ clusterId: string }>();
  const { data: clusters } = useReconciliationClusters();

  return (
    // AppLayout's <main> is overflow-hidden -- every routed page owns its
    // own scroll container, or content past the viewport is unreachable.
    <div className="flex-1 h-full overflow-y-auto">
      <div className="space-y-6 max-w-4xl mx-auto p-6 lg:p-10">
        <Button variant="ghost" size="sm" onClick={() => navigate('/reconciliation')}>
          <ArrowLeft className="w-4 h-4 mr-1" aria-hidden="true" /> Back
        </Button>

        <ClusterResolutionPanel
          clusterId={clusterId}
          queueClusters={clusters}
          onNavigate={(id) => navigate(`/reconciliation/${id}`)}
          onResolved={() => navigate('/reconciliation')}
        />
      </div>
    </div>
  );
}
