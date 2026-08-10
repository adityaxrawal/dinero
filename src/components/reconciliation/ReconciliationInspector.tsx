/**
 * Inspector panel for a single reconciliation cluster.
 */
import { X } from 'lucide-react';
import { useQueryClient } from '@tanstack/react-query';
import { cn } from '@/lib/utils';
import ClusterResolutionPanel from '@/components/reconciliation/ClusterResolutionPanel';
import { queryKeys } from '@/lib/queryKeys';
import type { ClusterRecord } from '@/lib/ipc';

interface ReconciliationInspectorProps {
  cluster: ClusterRecord | undefined;
  onClose: () => void;
  inline?: boolean;
  queueClusters?: ClusterRecord[];
  onNavigate?: (clusterId: string) => void;
}

/** Inspector panel for a single reconciliation cluster. */
export default function ReconciliationInspector({
  cluster,
  onClose,
  inline = false,
  queueClusters,
  onNavigate,
}: ReconciliationInspectorProps) {
  const queryClient = useQueryClient();
  const isOpen = !!cluster;

  const asideClasses = cn(
    !inline && 'inspector-panel',
    !inline && !isOpen && 'closed',
    inline && 'w-full h-full flex flex-col',
    !inline && 'flex-shrink-0'
  );

  const asideStyle = inline
    ? { backgroundColor: '#F8E7C9' }
    : { width: isOpen ? 'var(--inspector-width)' : 0 };

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
      <div
        className={cn(
          'flex items-center justify-end p-3 flex-shrink-0',
          !inline && 'border-b border-[#064E3B]/10'
        )}
      >
        <button
          type="button"
          className="w-8 h-8 flex items-center justify-center rounded-lg transition-colors hover:bg-[#064E3B]/10 text-[#064E3B]/60 hover:text-[#064E3B]"
          onClick={onClose}
          aria-label="Close inspector"
        >
          <X className="w-5 h-5" />
        </button>
      </div>

      <div className={cn('flex-1 overflow-y-auto', inline ? 'px-4 md:px-8 pb-4 max-w-3xl w-full' : 'p-4')}>
        <ClusterResolutionPanel
          clusterId={cluster.id}
          initialCluster={cluster}
          queueClusters={queueClusters}
          onNavigate={onNavigate}
          onResolved={() => {
            onClose();
            queryClient.invalidateQueries({ queryKey: queryKeys.reconciliation.unresolved() });
          }}
        />
      </div>
    </aside>
  );
}
