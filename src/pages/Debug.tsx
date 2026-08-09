import { useSearchParams } from 'react-router-dom';
import { SidebarNavItem } from '@/components/ui/sidebar-nav-item';
import { Server, Activity, AlertCircle, Link, Shield, Gauge } from 'lucide-react';

import { ParseErrorViewer } from '../components/debug/ParseErrorViewer';
import { UnprocessedStatementViewer } from '../components/debug/UnprocessedStatementViewer';
import { ReconciliationClusterViewer } from '../components/debug/ReconciliationClusterViewer';
import { AuditLogViewer } from '../components/debug/AuditLogViewer';
import { ReleaseReadinessViewer } from '../components/debug/ReleaseReadinessViewer';
import { useDebugMetrics } from './debug/useDebugMetrics';
import PipelineSection from './debug/PipelineSection';
import SystemMetricsSection from './debug/SystemMetricsSection';

const TABS = [
  { id: 'pipeline', label: 'Pipeline State', icon: <Activity size={16} /> },
  { id: 'extraction', label: 'Extraction Issues', icon: <AlertCircle size={16} /> },
  { id: 'reconciliation', label: 'Reconciliation', icon: <Link size={16} /> },
  { id: 'audit', label: 'Audit Log', icon: <Shield size={16} /> },
  { id: 'system', label: 'System Health', icon: <Server size={16} /> },
  // F15 fix: distinguishes locally-verifiable metrics from the
  // out-of-repo Licensing Backend admin surface.
  { id: 'release-readiness', label: 'Release Readiness', icon: <Gauge size={16} /> },
];

export default function Debug() {
  const [searchParams, setSearchParams] = useSearchParams();
  const activeTab = searchParams.get('section') || 'pipeline';
  const setActiveTab = (section: string) => setSearchParams({ section });
  const debug = useDebugMetrics();

  return (
    <div className="flex h-full w-full overflow-hidden">
      {/* ── Column 2: Navigation (Debug) ─────────────────────────────────── */}
      <div
        className="flex-shrink-0 flex flex-col h-full border-r border-[#064E3B]/20"
        style={{ width: '320px', backgroundColor: 'var(--bg-canvas)' }}
      >
        <div className="flex flex-col gap-3 px-4 py-3 flex-shrink-0 border-b border-[#064E3B]/10">
          <h1 className="text-[14px] font-semibold text-[#064E3B] tracking-tight">Debug Console</h1>
        </div>

        <div className="flex-1 overflow-y-auto py-2">
          <nav className="flex flex-col gap-1">
            {TABS.map((tab) => (
              <SidebarNavItem
                key={tab.id}
                isSelected={activeTab === tab.id}
                onClick={() => setActiveTab(tab.id)}
                icon={tab.icon}
                label={tab.label}
              />
            ))}
          </nav>
        </div>
      </div>

      {/* ── Column 3: Content Area ────────────────────────────────────────── */}
      <div className="flex-1 h-full bg-[#F8E7C9] relative overflow-y-auto p-8 lg:p-12 text-[#064E3B]">
        <div className="max-w-4xl mx-auto space-y-12">
          {activeTab === 'pipeline' && <PipelineSection debug={debug} />}

          {activeTab === 'extraction' && (
            <div className="animate-in fade-in duration-300 flex flex-col gap-8">
              <ParseErrorViewer />
              <div className="h-px w-full bg-[#064E3B]/10" />
              <UnprocessedStatementViewer />
            </div>
          )}

          {activeTab === 'reconciliation' && (
            <div className="animate-in fade-in duration-300 flex flex-col gap-8">
              <ReconciliationClusterViewer />
            </div>
          )}

          {activeTab === 'audit' && (
            <div className="animate-in fade-in duration-300">
              <AuditLogViewer />
            </div>
          )}

          {activeTab === 'release-readiness' && (
            <div className="animate-in fade-in duration-300">
              <ReleaseReadinessViewer metrics={debug.metrics} />
            </div>
          )}

          {activeTab === 'system' && (
            <SystemMetricsSection
              metrics={debug.metrics}
              ram={debug.ram}
              onRefresh={debug.refresh}
            />
          )}
        </div>
      </div>
    </div>
  );
}
