import { useState, useMemo } from 'react';
import { useSearchParams } from 'react-router-dom';
import { ShieldAlert, HelpCircle, Loader2, CheckCircle2, Layers } from 'lucide-react';

import { useReconciliationClusters } from '@/hooks/queries/useReconciliationClusters';
import { useUnassignedTransactions } from '@/hooks/queries/useUnassignedTransactions';
import ReconciliationInspector from '@/components/reconciliation/ReconciliationInspector';
import UnassignedInspector from '@/components/reconciliation/UnassignedInspector';
import { cn } from '@/lib/utils';

export default function Reconciliation() {
  const [searchParams, setSearchParams] = useSearchParams();
  const currentSection = searchParams.get('section') || 'clusters';
  const setSection = (section: string) => setSearchParams({ section });

  const { data: clusters = [], isLoading: clustersLoading } = useReconciliationClusters();
  const { data: unassigned = [], isLoading: unassignedLoading } = useUnassignedTransactions();
  
  const [selectedClusterId, setSelectedClusterId] = useState<string | null>(null);
  const [selectedUnassignedId, setSelectedUnassignedId] = useState<string | null>(null);

  const isLoading = clustersLoading || unassignedLoading;
  const allCaughtUp = !isLoading && clusters.length === 0 && unassigned.length === 0;

  const selectedCluster = useMemo(
    () => clusters.find((c) => c.id === selectedClusterId),
    [clusters, selectedClusterId]
  );

  const selectedUnassigned = useMemo(
    () => unassigned.find((u) => u.id === selectedUnassignedId),
    [unassigned, selectedUnassignedId]
  );

  const SECTIONS = [
    { id: 'clusters', label: 'Pending Clusters', icon: ShieldAlert, badge: clusters.length },
    { id: 'unassigned', label: 'Unassigned', icon: HelpCircle, badge: unassigned.length },
  ] as const;

  return (
    <div className="flex h-full w-full overflow-hidden">
      {/* ── Column 2: Master List (Reconciliation) ─────────────────────────────────── */}
      <div 
        className="flex-shrink-0 flex flex-col h-full border-r border-[#064E3B]/20"
        style={{ width: '320px', backgroundColor: 'var(--bg-canvas)' }}
      >
        {/* Header bar */}
        <div
          className="flex flex-col gap-3 px-4 py-3 flex-shrink-0 border-b border-[#064E3B]/10"
        >
          <div className="flex items-center justify-between">
            <h1 className="text-[14px] font-semibold text-[#064E3B] tracking-tight">
              Reconciliation
            </h1>
          </div>
          
          <div className="flex gap-1 overflow-x-auto pb-1" role="tablist">
            {SECTIONS.map((s) => (
              <button
                key={s.id}
                role="tab"
                aria-selected={currentSection === s.id}
                onClick={() => setSection(s.id)}
                className={cn(
                  "px-3 py-1.5 text-[12px] font-medium rounded-full transition-colors whitespace-nowrap flex items-center gap-1.5",
                  currentSection === s.id
                    ? "bg-[#064E3B] text-[#F8E7C9]"
                    : "text-[#064E3B]/70 hover:bg-[#064E3B]/10"
                )}
              >
                <s.icon className="w-3.5 h-3.5" />
                {s.label}
                {s.badge > 0 && (
                  <span className={cn(
                    "ml-1 px-1.5 py-0.5 rounded-full text-[10px] font-bold",
                    currentSection === s.id ? "bg-[#F8E7C9]/20 text-[#F8E7C9]" : "bg-[#064E3B]/10 text-[#064E3B]"
                  )}>
                    {s.badge}
                  </span>
                )}
              </button>
            ))}
          </div>
        </div>

        {/* List items */}
        <div className="flex-1 overflow-y-auto">
          {isLoading ? (
            <div className="flex flex-col items-center justify-center h-40 gap-2">
              <Loader2 className="w-4 h-4 animate-spin text-[#064E3B]/50" />
              <span className="text-xs text-[#064E3B]/50">Loading queue...</span>
            </div>
          ) : allCaughtUp ? (
            <div className="flex flex-col items-center justify-center text-center p-8 h-full opacity-60">
              <div className="w-12 h-12 rounded-full flex items-center justify-center mb-4 bg-[#064E3B]/10 text-[#064E3B]">
                <CheckCircle2 className="w-6 h-6" />
              </div>
              <h3 className="text-sm font-semibold mb-1 text-[#064E3B]">All Caught Up!</h3>
              <p className="text-[11px] text-[#064E3B]">No pending clusters requiring review.</p>
            </div>
          ) : (
            <div className="py-2">
              {currentSection === 'clusters' && (
                clusters.length === 0 ? (
                  <p className="text-[12px] text-center p-4 text-[#064E3B]/60">No pending clusters.</p>
                ) : (
                  <div className="flex flex-col gap-1">
                    {clusters.map((cluster) => {
                      const isSelected = selectedClusterId === cluster.id;
                      const title = cluster.reason.startsWith('Ambiguous match')
                        ? cluster.reason.substring(15).trim()
                        : cluster.reason;

                      return (
                        <button
                          key={cluster.id}
                          onClick={() => setSelectedClusterId(isSelected ? null : cluster.id)}
                          className={cn(
                            "flex flex-col w-full text-left px-4 py-2.5 mx-2 rounded-md transition-colors max-w-[calc(100%-16px)] cursor-pointer select-none",
                            isSelected
                              ? "bg-[#064E3B] text-[#F8E7C9]"
                              : "hover:bg-[#064E3B]/5 text-[#064E3B]"
                          )}
                        >
                          <div className="flex items-center gap-1.5 mb-1">
                            <span className={cn(
                              "text-[9px] font-bold px-1.5 py-0.5 rounded-sm uppercase tracking-wider",
                              isSelected ? "bg-[#F8E7C9]/20 text-[#F8E7C9]" : "bg-amber-500/20 text-amber-700"
                            )}>
                              Ambiguous
                            </span>
                          </div>
                          <p className={cn("text-[13px] font-semibold truncate mb-0.5", isSelected ? "text-white" : "text-[#064E3B]")}>
                            {title || 'Match requires review'}
                          </p>
                          <p className={cn("text-[11px] truncate opacity-70 font-medium")}>
                            {cluster.members_count} member(s)
                          </p>
                        </button>
                      );
                    })}
                  </div>
                )
              )}

              {currentSection === 'unassigned' && (
                unassigned.length === 0 ? (
                  <p className="text-[12px] text-center p-4 text-[#064E3B]/60">No unassigned transactions.</p>
                ) : (
                  <div className="flex flex-col gap-1">
                    {unassigned.map((item) => {
                      const isSelected = selectedUnassignedId === item.id;
                      const title = item.reason === 'extraction_failed' ? 'Failed to Extract' : (item.reason === 'issuer_name_not_found' ? 'Unknown Instrument' : item.reason || 'Unresolved');
                      return (
                        <button
                          key={item.id}
                          onClick={() => setSelectedUnassignedId(isSelected ? null : item.id)}
                          className={cn(
                            "flex flex-col w-full text-left px-4 py-2.5 mx-2 rounded-md transition-colors max-w-[calc(100%-16px)] cursor-pointer select-none",
                            isSelected
                              ? "bg-[#064E3B] text-[#F8E7C9]"
                              : "hover:bg-[#064E3B]/5 text-[#064E3B]"
                          )}
                        >
                          <div className="flex items-center gap-1.5 mb-1">
                            <span className={cn(
                              "text-[9px] font-bold px-1.5 py-0.5 rounded-sm uppercase tracking-wider",
                              isSelected ? "bg-[#F8E7C9]/20 text-[#F8E7C9]" : "bg-red-500/20 text-red-700"
                            )}>
                              Action Required
                            </span>
                          </div>
                          <p className={cn("text-[13px] font-semibold truncate mb-0.5", isSelected ? "text-white" : "text-[#064E3B]")}>
                            {title}
                          </p>
                          <p className={cn("text-[11px] truncate opacity-70 font-medium")}>
                            Obs: {item.observation_id.substring(0,8)}...
                          </p>
                        </button>
                      );
                    })}
                  </div>
                )
              )}
            </div>
          )}
        </div>
      </div>

      {/* ── Column 3: Inspector Panel ─────────────────────────────────── */}
      <div className="flex-1 h-full bg-[#F8E7C9] relative overflow-hidden flex flex-col justify-center">
        {selectedClusterId && currentSection === 'clusters' ? (
          <div className="w-full h-full flex flex-col">
            <ReconciliationInspector
              cluster={selectedCluster}
              onClose={() => setSelectedClusterId(null)}
              inline={true}
            />
          </div>
        ) : selectedUnassignedId && currentSection === 'unassigned' ? (
          <div className="w-full h-full flex flex-col">
            <UnassignedInspector
              record={selectedUnassigned}
              onClose={() => setSelectedUnassignedId(null)}
              inline={true}
            />
          </div>
        ) : (
          <div className="flex-1 flex flex-col items-center justify-center h-full opacity-30">
            <div className="w-12 h-12 border-2 border-[#064E3B] rounded-xl mb-4 border-dashed flex items-center justify-center">
              <Layers className="w-6 h-6 text-[#064E3B]" />
            </div>
            <p className="text-[#064E3B] font-medium text-sm">
              {currentSection === 'clusters' ? 'Select a cluster to resolve' : 'Select an item to view details'}
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
