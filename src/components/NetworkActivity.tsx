/**
 * Displays the outbound network activity log.
 *
 * The evidence behind the privacy disclosure, showing what actually left the
 * machine rather than only what was promised.
 */
import { useState, useEffect } from 'react';
import { Activity, RefreshCcw } from 'lucide-react';
import { API } from '../lib/ipc';
import { OUTBOUND_CHANNEL_DISCLOSURE } from '../constants/privacy';

interface NetworkLogEntry {
  id: string;
  timestamp: string;
  method: string;
  domain: string;
  url_redacted: string;
  bytes_sent: number | null;
  bytes_received: number | null;
  status_code: number | null;
}

/** Displays the outbound network activity log. */
export default function NetworkActivity() {
  const [logs, setLogs] = useState<NetworkLogEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  /** Loads a page of activity entries. */
  const fetchLogs = async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await API.network.getActivityList<NetworkLogEntry>();
      setLogs(data);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Failed to fetch network activity.');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchLogs();
  }, []);

  return (
    <div className="space-y-5">
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center gap-2">
          <Activity className="w-5 h-5 text-emerald-ink" />
          <h3 className="text-xl font-bold text-emerald-ink">Network Activity</h3>
        </div>
        <button
          className="h-8 px-3 text-[12px] font-semibold rounded-lg border border-emerald-ink/20 text-emerald-ink hover:bg-emerald-ink/5 transition-colors flex items-center gap-1.5 disabled:opacity-50"
          onClick={fetchLogs}
          disabled={loading}
        >
          <RefreshCcw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
          Refresh
        </button>
      </div>

      <div className="p-4 rounded-xl border border-emerald-ink/10 bg-emerald-ink/5 mb-4">
        <p className="text-[13px] font-bold text-emerald-ink mb-2">Outbound Channels Disclosure:</p>
        <ul className="text-[12px] font-medium text-emerald-ink/85 list-disc pl-5 space-y-1">
          {OUTBOUND_CHANNEL_DISCLOSURE.map((item, i) => (
            <li key={i}>{item}</li>
          ))}
        </ul>
      </div>

      {error ? (
        <div className="text-[13px] font-medium text-red-600 mb-4">{error}</div>
      ) : loading && logs.length === 0 ? (
        <div className="flex items-center gap-2 text-[13px] font-medium text-emerald-ink/85">
          <div className="w-4 h-4 border-2 border-emerald-ink/20 border-t-emerald-ink rounded-full animate-spin" />
          Loading network activity...
        </div>
      ) : logs.length === 0 ? (
        <div className="text-[13px] font-medium text-emerald-ink/85">
          No outbound requests recorded yet.
        </div>
      ) : (
        <div className="overflow-x-auto rounded-xl border border-emerald-ink/10 bg-champagne/50">
          <table className="w-full text-left border-collapse">
            <thead>
              <tr className="border-b border-emerald-ink/10 text-[11px] font-bold uppercase tracking-wider text-emerald-ink/85">
                <th className="p-3">Timestamp</th>
                <th className="p-3">Method</th>
                <th className="p-3">Domain</th>
                <th className="p-3">URL (Redacted)</th>
                <th className="p-3">Sent (B)</th>
                <th className="p-3">Recv (B)</th>
                <th className="p-3">Status</th>
              </tr>
            </thead>
            <tbody>
              {logs.map((log) => (
                <tr
                  key={log.id}
                  className="border-b border-emerald-ink/10 last:border-0 hover:bg-emerald-ink/5"
                >
                  <td className="p-3 text-[12px] font-medium text-emerald-ink/80">
                    {new Date(log.timestamp).toLocaleString()}
                  </td>
                  <td className="p-3 text-[12px] font-bold text-emerald-ink">{log.method}</td>
                  <td className="p-3 text-[12px] font-medium text-emerald-ink/80">{log.domain}</td>
                  <td
                    className="p-3 text-[12px] font-mono text-emerald-ink/85 truncate max-w-[200px]"
                    title={log.url_redacted}
                  >
                    {log.url_redacted}
                  </td>
                  <td className="p-3 text-[12px] font-mono text-emerald-ink/80">
                    {log.bytes_sent ?? '-'}
                  </td>
                  <td className="p-3 text-[12px] font-mono text-emerald-ink/80">
                    {log.bytes_received ?? '-'}
                  </td>
                  <td className="p-3 text-[12px] font-bold text-emerald-ink">
                    {log.status_code ?? '-'}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
