import { useEffect, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { cn } from '@/lib/utils';
import { SidebarNavItem } from '@/components/ui/sidebar-nav-item';
import { API, DebugMetrics } from '../lib/ipc';
import {
  Database,
  FileText,
  ListOrdered,
  Server,
  RefreshCw,
  Activity,
  AlertCircle,
  Link,
  Shield,
  Settings,
  Pause,
  Play,
  Gauge,
} from 'lucide-react';

import { ParseErrorViewer } from '../components/debug/ParseErrorViewer';
import { UnprocessedStatementViewer } from '../components/debug/UnprocessedStatementViewer';
import { ReconciliationClusterViewer } from '../components/debug/ReconciliationClusterViewer';
import { PatternRuleHealthViewer } from '../components/debug/PatternRuleHealthViewer';
import { AuditLogViewer } from '../components/debug/AuditLogViewer';
import { ReleaseReadinessViewer } from '../components/debug/ReleaseReadinessViewer';

interface PipelineState {
  gmail_poll_paused: boolean;
  scan_queue_paused: boolean;
}

export default function Debug() {
  const [searchParams, setSearchParams] = useSearchParams();
  const activeTab = searchParams.get('section') || 'pipeline';
  const setActiveTab = (section: string) => setSearchParams({ section });
  const [metrics, setMetrics] = useState<DebugMetrics | null>(null);
  const [pipelineState, setPipelineState] = useState<PipelineState | null>(null);
  const [ram, setRam] = useState<number | null>(null);

  const fetchGlobalMetrics = () => {
    API.dev.getMetrics().then(setMetrics).catch(console.error);
    API.dev.checkSystemRam().then(setRam).catch(console.error);
    API.debug.getPipelineState().then(setPipelineState).catch(console.error);
  };

  useEffect(() => {
    fetchGlobalMetrics();
    const interval = setInterval(fetchGlobalMetrics, 15000);
    return () => clearInterval(interval);
  }, []);

  const toggleGmailPoll = async () => {
    if (!pipelineState) return;
    const newState = !pipelineState.gmail_poll_paused;
    await API.debug.setGmailPollPaused(newState);
    fetchGlobalMetrics();
  };

  const toggleScanQueue = async () => {
    if (!pipelineState) return;
    const newState = !pipelineState.scan_queue_paused;
    await API.debug.setScanQueuePaused(newState);
    fetchGlobalMetrics();
  };

  const tabs = [
    { id: 'pipeline', label: 'Pipeline State', icon: <Activity size={16} /> },
    { id: 'extraction', label: 'Extraction Issues', icon: <AlertCircle size={16} /> },
    { id: 'reconciliation', label: 'Reconciliation', icon: <Link size={16} /> },
    { id: 'audit', label: 'Audit Log', icon: <Shield size={16} /> },
    { id: 'system', label: 'System Health', icon: <Server size={16} /> },
    // F15 fix: distinguishes locally-verifiable metrics from the
    // out-of-repo Licensing Backend admin surface.
    { id: 'release-readiness', label: 'Release Readiness', icon: <Gauge size={16} /> },
  ];

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
            {tabs.map((tab) => {
              const isSelected = activeTab === tab.id;
              return (
                <SidebarNavItem
                  key={tab.id}
                  isSelected={isSelected}
                  onClick={() => setActiveTab(tab.id)}
                  icon={tab.icon}
                  label={tab.label}
                />
              );
            })}
          </nav>
        </div>
      </div>

      {/* ── Column 3: Content Area ────────────────────────────────────────── */}
      <div className="flex-1 h-full bg-[#F8E7C9] relative overflow-y-auto p-8 lg:p-12 text-[#064E3B]">
        <div className="max-w-4xl mx-auto space-y-12">
          {activeTab === 'pipeline' && (
            <div className="animate-in fade-in duration-300 flex flex-col gap-6">
              <h2 className="text-xl font-bold">Pipeline Controls</h2>
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div className="bg-[#F8E7C9]/50 border border-[#064E3B]/10 rounded-xl p-6 flex flex-col gap-4">
                  <div className="flex justify-between items-center">
                    <h3 className="font-semibold flex items-center gap-2 text-[14px]">
                      <Server size={18} /> Transaction Polling
                    </h3>
                    {pipelineState?.gmail_poll_paused ? (
                      <span className="text-[10px] uppercase tracking-wider px-2 py-0.5 bg-amber-500/20 text-amber-700 rounded-sm font-bold">
                        PAUSED
                      </span>
                    ) : (
                      <span className="text-[10px] uppercase tracking-wider px-2 py-0.5 bg-[#064E3B]/10 text-[#064E3B] rounded-sm font-bold">
                        RUNNING
                      </span>
                    )}
                  </div>
                  <p className="text-[12px] text-[#064E3B]/70">
                    Controls the background polling of Gmail for new transaction emails.
                  </p>
                  <button
                    className={cn(
                      'h-8 rounded-lg text-[13px] font-semibold flex items-center justify-center gap-2 transition-colors mt-auto',
                      pipelineState?.gmail_poll_paused
                        ? 'bg-[#064E3B] text-[#F8E7C9] hover:bg-[#064E3B]/90'
                        : 'bg-amber-500/20 text-amber-700 hover:bg-amber-500/30'
                    )}
                    onClick={toggleGmailPoll}
                  >
                    {pipelineState?.gmail_poll_paused ? (
                      <>
                        <Play size={14} /> Resume Polling
                      </>
                    ) : (
                      <>
                        <Pause size={14} /> Pause Polling
                      </>
                    )}
                  </button>
                </div>

                <div className="bg-[#F8E7C9]/50 border border-[#064E3B]/10 rounded-xl p-6 flex flex-col gap-4">
                  <div className="flex justify-between items-center">
                    <h3 className="font-semibold flex items-center gap-2 text-[14px]">
                      <Settings size={18} /> Historical Scan Queue
                    </h3>
                    {pipelineState?.scan_queue_paused ? (
                      <span className="text-[10px] uppercase tracking-wider px-2 py-0.5 bg-amber-500/20 text-amber-700 rounded-sm font-bold">
                        PAUSED
                      </span>
                    ) : (
                      <span className="text-[10px] uppercase tracking-wider px-2 py-0.5 bg-[#064E3B]/10 text-[#064E3B] rounded-sm font-bold">
                        RUNNING
                      </span>
                    )}
                  </div>
                  <p className="text-[12px] text-[#064E3B]/70">
                    Controls the processing of the historical scan queue for fetching emails.
                  </p>
                  <button
                    className={cn(
                      'h-8 rounded-lg text-[13px] font-semibold flex items-center justify-center gap-2 transition-colors mt-auto',
                      pipelineState?.scan_queue_paused
                        ? 'bg-[#064E3B] text-[#F8E7C9] hover:bg-[#064E3B]/90'
                        : 'bg-amber-500/20 text-amber-700 hover:bg-amber-500/30'
                    )}
                    onClick={toggleScanQueue}
                  >
                    {pipelineState?.scan_queue_paused ? (
                      <>
                        <Play size={14} /> Resume Scan
                      </>
                    ) : (
                      <>
                        <Pause size={14} /> Pause Scan
                      </>
                    )}
                  </button>
                </div>
              </div>
            </div>
          )}

          {activeTab === 'extraction' && (
            <div className="animate-in fade-in duration-300 flex flex-col gap-8">
              <ParseErrorViewer />
              <div className="h-px w-full bg-[#064E3B]/10" />
              <UnprocessedStatementViewer />
            </div>
          )}

          {activeTab === 'reconciliation' && (
            <div className="animate-in fade-in duration-300 flex flex-col gap-8">
              <PatternRuleHealthViewer />
              <div className="h-px w-full bg-[#064E3B]/10" />
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
              <ReleaseReadinessViewer metrics={metrics} />
            </div>
          )}

          {activeTab === 'system' && (
            <div className="animate-in fade-in duration-300 flex flex-col gap-6">
              <div className="flex justify-between items-center">
                <h2 className="text-xl font-bold">System Metrics</h2>
                <button
                  className="h-8 px-3 rounded-lg text-[13px] font-semibold flex items-center gap-2 bg-[#064E3B]/10 text-[#064E3B] hover:bg-[#064E3B]/20 transition-colors"
                  onClick={fetchGlobalMetrics}
                >
                  <RefreshCw size={14} /> Refresh
                </button>
              </div>
              {metrics ? (
                <>
                  <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-6 gap-4">
                    <div className="bg-[#F8E7C9]/50 border border-[#064E3B]/10 rounded-xl p-4 flex flex-col gap-2">
                      <ListOrdered size={20} className="text-[#064E3B]/70" />
                      <div>
                        <p className="text-[11px] font-semibold uppercase tracking-wider text-[#064E3B]/60">
                          Transactions
                        </p>
                        <h3 className="text-2xl font-bold text-[#064E3B]">
                          {metrics.total_transactions}
                        </h3>
                      </div>
                    </div>
                    <div className="bg-[#F8E7C9]/50 border border-[#064E3B]/10 rounded-xl p-4 flex flex-col gap-2">
                      <FileText size={20} className="text-[#064E3B]/70" />
                      <div>
                        <p className="text-[11px] font-semibold uppercase tracking-wider text-[#064E3B]/60">
                          Statements
                        </p>
                        <h3 className="text-2xl font-bold text-[#064E3B]">
                          {metrics.total_statements}
                        </h3>
                      </div>
                    </div>
                    <div className="bg-[#F8E7C9]/50 border border-[#064E3B]/10 rounded-xl p-4 flex flex-col gap-2">
                      <Database size={20} className="text-[#064E3B]/70" />
                      <div>
                        <p className="text-[11px] font-semibold uppercase tracking-wider text-[#064E3B]/60">
                          Unresolved
                        </p>
                        <h3 className="text-2xl font-bold text-[#064E3B]">
                          {metrics.unresolved_clusters}
                        </h3>
                      </div>
                    </div>
                    <div className="bg-[#F8E7C9]/50 border border-[#064E3B]/10 rounded-xl p-4 flex flex-col gap-2">
                      <Activity size={20} className="text-[#064E3B]/70" />
                      <div>
                        <p className="text-[11px] font-semibold uppercase tracking-wider text-[#064E3B]/60">
                          LLM Fallback
                        </p>
                        <h3 className="text-2xl font-bold text-[#064E3B]">
                          {(metrics.llm_fallback_rate * 100).toFixed(1)}%
                        </h3>
                      </div>
                    </div>
                    <div className="bg-[#F8E7C9]/50 border border-[#064E3B]/10 rounded-xl p-4 flex flex-col gap-2">
                      <Settings size={20} className="text-[#064E3B]/70" />
                      <div>
                        <p className="text-[11px] font-semibold uppercase tracking-wider text-[#064E3B]/60">
                          Queue Depth
                        </p>
                        <h3 className="text-2xl font-bold text-[#064E3B]">{metrics.queue_depth}</h3>
                      </div>
                    </div>
                    <div className="bg-[#F8E7C9]/50 border border-[#064E3B]/10 rounded-xl p-4 flex flex-col gap-2">
                      <Server size={20} className="text-[#064E3B]/70" />
                      <div>
                        <p className="text-[11px] font-semibold uppercase tracking-wider text-[#064E3B]/60">
                          Available RAM
                        </p>
                        <h3 className="text-2xl font-bold text-[#064E3B]">
                          {ram ? `${ram} GB` : '...'}
                        </h3>
                      </div>
                    </div>
                  </div>

                  <div className="grid grid-cols-1 md:grid-cols-2 gap-6 mt-6">
                    <div className="bg-[#F8E7C9]/50 border border-[#064E3B]/10 rounded-xl p-6">
                      <h3 className="text-[14px] font-bold mb-4">Extraction Layer Distribution</h3>
                      <div className="flex flex-col gap-2">
                        {Object.entries(metrics.extraction_layer_distribution || {}).map(
                          ([layer, count]) => (
                            <div
                              key={layer}
                              className="flex justify-between items-center py-2 border-b border-[#064E3B]/10 last:border-0"
                            >
                              <span className="text-[13px] font-medium">{layer}</span>
                              <span className="text-[13px] font-semibold text-[#064E3B]/70">
                                {count}
                              </span>
                            </div>
                          )
                        )}
                        {Object.keys(metrics.extraction_layer_distribution || {}).length === 0 && (
                          <p className="text-[13px] text-[#064E3B]/60">No data available.</p>
                        )}
                      </div>
                    </div>

                    <div className="bg-[#F8E7C9]/50 border border-[#064E3B]/10 rounded-xl p-6">
                      <h3 className="text-[14px] font-bold mb-4">Reconciliation Decisions</h3>
                      <div className="flex flex-col gap-2">
                        {Object.entries(metrics.reconciliation_decision_distribution || {}).map(
                          ([decision, count]) => (
                            <div
                              key={decision}
                              className="flex justify-between items-center py-2 border-b border-[#064E3B]/10 last:border-0"
                            >
                              <span className="text-[13px] font-medium">{decision}</span>
                              <span className="text-[13px] font-semibold text-[#064E3B]/70">
                                {count}
                              </span>
                            </div>
                          )
                        )}
                        {Object.keys(metrics.reconciliation_decision_distribution || {}).length ===
                          0 && <p className="text-[13px] text-[#064E3B]/60">No data available.</p>}
                      </div>
                    </div>
                  </div>
                </>
              ) : (
                <div className="text-[#064E3B]/60 text-[13px]">Loading metrics...</div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
