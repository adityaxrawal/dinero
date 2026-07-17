import { useEffect, useState } from 'react';
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
  Gauge
} from 'lucide-react';

import { ParseErrorViewer } from '../components/debug/ParseErrorViewer';
import { UnprocessedStatementViewer } from '../components/debug/UnprocessedStatementViewer';
import { ReconciliationClusterViewer } from '../components/debug/ReconciliationClusterViewer';
import { PatternRuleHealthViewer } from '../components/debug/PatternRuleHealthViewer';
import { AuditLogViewer } from '../components/debug/AuditLogViewer';
import { ReleaseReadinessViewer } from '../components/debug/ReleaseReadinessViewer';

export default function Debug() {
  const [activeTab, setActiveTab] = useState('pipeline');
  const [metrics, setMetrics] = useState<DebugMetrics | null>(null);
  const [pipelineState, setPipelineState] = useState<any>(null);
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
    <div className="animate-fade-in flex flex-col gap-6">
      <div className="flex justify-between items-center">
        <div>
          <h1 className="heading-lg">Debug Console</h1>
          <p className="text-sm text-muted-foreground mt-1">
            View operational health, pipeline metrics, and system logs.
          </p>
        </div>
      </div>

      <div className="flex gap-2 border-b border-[var(--border-color)] overflow-x-auto pb-px">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={`flex items-center gap-2 px-4 py-2 border-b-2 transition-colors whitespace-nowrap ${
              activeTab === tab.id
                ? 'border-accent text-accent font-medium bg-accent/5 rounded-t-md'
                : 'border-transparent text-muted-foreground hover:text-foreground hover:bg-white/5 rounded-t-md'
            }`}
          >
            {tab.icon}
            {tab.label}
          </button>
        ))}
      </div>

      <div className="min-h-[400px]">
        {activeTab === 'pipeline' && (
          <div className="flex flex-col gap-6">
            <h2 className="heading-md">Pipeline Controls</h2>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div className="glass-panel p-6 flex flex-col gap-4">
                <div className="flex justify-between items-center">
                  <h3 className="font-medium flex items-center gap-2"><Server size={18}/> Transaction Polling</h3>
                  {pipelineState?.gmail_poll_paused ? (
                    <span className="text-xs px-2 py-1 bg-yellow-500/20 text-yellow-500 rounded-full font-bold">PAUSED</span>
                  ) : (
                    <span className="text-xs px-2 py-1 bg-green-500/20 text-green-700 rounded-full font-bold">RUNNING</span>
                  )}
                </div>
                <p className="text-sm text-muted-foreground">Controls the background polling of Gmail for new transaction emails.</p>
                <button 
                  className={`btn ${pipelineState?.gmail_poll_paused ? 'bg-green-600 hover:bg-green-700' : 'bg-yellow-600 hover:bg-yellow-700'} text-white w-full flex justify-center items-center gap-2`}
                  onClick={toggleGmailPoll}
                >
                  {pipelineState?.gmail_poll_paused ? <><Play size={16}/> Resume Polling</> : <><Pause size={16}/> Pause Polling</>}
                </button>
              </div>

              <div className="glass-panel p-6 flex flex-col gap-4">
                <div className="flex justify-between items-center">
                  <h3 className="font-medium flex items-center gap-2"><Settings size={18}/> Historical Scan Queue</h3>
                  {pipelineState?.scan_queue_paused ? (
                    <span className="text-xs px-2 py-1 bg-yellow-500/20 text-yellow-500 rounded-full font-bold">PAUSED</span>
                  ) : (
                    <span className="text-xs px-2 py-1 bg-green-500/20 text-green-700 rounded-full font-bold">RUNNING</span>
                  )}
                </div>
                <p className="text-sm text-muted-foreground">Controls the processing of the historical scan queue for fetching emails.</p>
                <button 
                  className={`btn ${pipelineState?.scan_queue_paused ? 'bg-green-600 hover:bg-green-700' : 'bg-yellow-600 hover:bg-yellow-700'} text-white w-full flex justify-center items-center gap-2`}
                  onClick={toggleScanQueue}
                >
                  {pipelineState?.scan_queue_paused ? <><Play size={16}/> Resume Scan</> : <><Pause size={16}/> Pause Scan</>}
                </button>
              </div>
            </div>
          </div>
        )}

        {activeTab === 'extraction' && (
          <div className="flex flex-col gap-8">
            <ParseErrorViewer />
            <div className="h-px w-full bg-[var(--border-color)]" />
            <UnprocessedStatementViewer />
          </div>
        )}

        {activeTab === 'reconciliation' && (
          <div className="flex flex-col gap-8">
            <PatternRuleHealthViewer />
            <div className="h-px w-full bg-[var(--border-color)]" />
            <ReconciliationClusterViewer />
          </div>
        )}

        {activeTab === 'audit' && (
          <AuditLogViewer />
        )}

        {activeTab === 'release-readiness' && (
          <ReleaseReadinessViewer metrics={metrics} />
        )}

        {activeTab === 'system' && (
          <div className="flex flex-col gap-6">
            <div className="flex justify-between items-center">
              <h2 className="heading-md">System Metrics</h2>
              <button className="btn btn-secondary text-sm flex items-center gap-2" onClick={fetchGlobalMetrics}>
                <RefreshCw size={14}/> Refresh
              </button>
            </div>
            {metrics ? (
              <>
                <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-6 gap-4">
                  <div className="glass-panel p-4 flex items-center gap-4">
                    <ListOrdered size={24} className="text-accent" />
                    <div>
                      <p className="text-xs text-muted-foreground">Transactions</p>
                      <h3 className="heading-md">{metrics.total_transactions}</h3>
                    </div>
                  </div>
                  <div className="glass-panel p-4 flex items-center gap-4">
                    <FileText size={24} className="text-accent" />
                    <div>
                      <p className="text-xs text-muted-foreground">Statements</p>
                      <h3 className="heading-md">{metrics.total_statements}</h3>
                    </div>
                  </div>
                  <div className="glass-panel p-4 flex items-center gap-4">
                    <Database size={24} className="text-accent" />
                    <div>
                      <p className="text-xs text-muted-foreground">Unresolved Clusters</p>
                      <h3 className="heading-md">{metrics.unresolved_clusters}</h3>
                    </div>
                  </div>
                  <div className="glass-panel p-4 flex items-center gap-4">
                    <Activity size={24} className="text-accent" />
                    <div>
                      <p className="text-xs text-muted-foreground">LLM Fallback</p>
                      <h3 className="heading-md">{(metrics.llm_fallback_rate * 100).toFixed(1)}%</h3>
                    </div>
                  </div>
                  <div className="glass-panel p-4 flex items-center gap-4">
                    <Settings size={24} className="text-accent" />
                    <div>
                      <p className="text-xs text-muted-foreground">Queue Depth</p>
                      <h3 className="heading-md">{metrics.queue_depth}</h3>
                    </div>
                  </div>
                  <div className="glass-panel p-4 flex items-center gap-4">
                    <Server size={24} className="text-accent" />
                    <div>
                      <p className="text-xs text-muted-foreground">Available RAM</p>
                      <h3 className="heading-md">{ram ? `${ram} GB` : '...'}</h3>
                    </div>
                  </div>
                </div>

                <div className="grid grid-cols-1 md:grid-cols-2 gap-6 mt-6">
                  <div className="glass-panel p-6">
                    <h3 className="heading-sm mb-4">Extraction Layer Distribution</h3>
                    <div className="flex flex-col gap-2">
                      {Object.entries(metrics.extraction_layer_distribution || {}).map(([layer, count]) => (
                        <div key={layer} className="flex justify-between items-center py-2 border-b border-[var(--border-color)] last:border-0">
                          <span className="text-sm font-medium">{layer}</span>
                          <span className="text-sm text-muted-foreground">{count}</span>
                        </div>
                      ))}
                      {Object.keys(metrics.extraction_layer_distribution || {}).length === 0 && (
                        <p className="text-sm text-muted-foreground">No data available.</p>
                      )}
                    </div>
                  </div>

                  <div className="glass-panel p-6">
                    <h3 className="heading-sm mb-4">Reconciliation Decisions</h3>
                    <div className="flex flex-col gap-2">
                      {Object.entries(metrics.reconciliation_decision_distribution || {}).map(([decision, count]) => (
                        <div key={decision} className="flex justify-between items-center py-2 border-b border-[var(--border-color)] last:border-0">
                          <span className="text-sm font-medium">{decision}</span>
                          <span className="text-sm text-muted-foreground">{count}</span>
                        </div>
                      ))}
                      {Object.keys(metrics.reconciliation_decision_distribution || {}).length === 0 && (
                        <p className="text-sm text-muted-foreground">No data available.</p>
                      )}
                    </div>
                  </div>
                </div>
              </>
            ) : (
              <div className="text-muted-foreground">Loading metrics...</div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
