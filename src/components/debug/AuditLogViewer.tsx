/**
 * Raw audit log viewer for the debug screen.
 */
import { useEffect, useState, useCallback } from 'react';
import { API } from '../../lib/ipc';
import { DebugTableLayout } from './DebugTableLayout';
import { Badge } from '../ui/badge';

interface AuditLog {
  id: string;
  created_at: string;
  action: string;
  resource_type: string;
  resource_id: string | null;
  actor_type: string;
  actor_id: string;
  after_json: unknown;
}

/** Raw audit log viewer. */
export function AuditLogViewer() {
  const [logs, setLogs] = useState<AuditLog[]>([]);
  const [loading, setLoading] = useState(true);
  const [resourceFilter, setResourceFilter] = useState<string>('');

  const fetchLogs = useCallback(async () => {
    setLoading(true);
    try {
      const data = await API.debug.fetchAuditLog(resourceFilter || undefined, 100, 0);
      setLogs(data);
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  }, [resourceFilter]);

  useEffect(() => {
    fetchLogs();
  }, [fetchLogs]);

  return (
    <DebugTableLayout
      title="Audit Log"
      onRefresh={fetchLogs}
      loading={loading}
      data={logs}
      loadingMessage="Loading audit logs..."
      emptyMessage="No audit logs found."
      headerActions={
        <select
          className="p-1 text-sm bg-transparent border border-[var(--border-color)] rounded text-foreground"
          value={resourceFilter}
          onChange={(e) => setResourceFilter(e.target.value)}
        >
          <option value="">All Resources</option>
          <option value="reconciliation_cluster">Clusters</option>
          <option value="spending_limits">Spending Limits</option>
          <option value="transaction">Transactions</option>
          <option value="instrument">Instruments</option>
        </select>
      }
      headers={
        <>
          <th className="p-2 text-sm font-medium text-muted-foreground">Time</th>
          <th className="p-2 text-sm font-medium text-muted-foreground">Action</th>
          <th className="p-2 text-sm font-medium text-muted-foreground">Resource</th>
          <th className="p-2 text-sm font-medium text-muted-foreground">Actor</th>
          <th className="p-2 text-sm font-medium text-muted-foreground">Details</th>
        </>
      }
      renderRow={(log) => (
        <tr key={log.id} className="border-b border-[var(--border-color)] last:border-0 align-top">
          <td className="p-2 text-sm whitespace-nowrap">
            {new Date(log.created_at).toLocaleString()}
          </td>
          <td className="p-2 text-sm">
            <Badge variant="outline">{log.action}</Badge>
          </td>
          <td className="p-2 text-sm">
            {log.resource_type} <br />
            <span className="text-xs text-muted-foreground font-mono">
              {log.resource_id?.substring(0, 8)}
            </span>
          </td>
          <td className="p-2 text-sm">
            {log.actor_type}:{log.actor_id}
          </td>
          <td className="p-2 text-sm max-w-[200px] overflow-hidden text-ellipsis whitespace-nowrap">
            {log.after_json ? JSON.stringify(log.after_json) : 'N/A'}
          </td>
        </tr>
      )}
    />
  );
}
