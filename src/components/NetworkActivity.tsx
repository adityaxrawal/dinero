import { useState, useEffect } from 'react';
import { Activity, RefreshCcw } from 'lucide-react';
import { API } from '../lib/ipc';
import { OUTBOUND_CHANNEL_DISCLOSURE } from '../constants/privacy';

export default function NetworkActivity() {
  const [logs, setLogs] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const fetchLogs = async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await API.network.getActivityList();
      setLogs(data);
    } catch (e: any) {
      setError(e.message || 'Failed to fetch network activity.');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchLogs();
  }, []);

  return (
    <div className="glass-panel" style={{ padding: '24px', marginBottom: '24px' }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: '16px' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
          <Activity className="text-primary" size={24} />
          <h3 className="heading-md">Network Activity</h3>
        </div>
        <button className="btn btn-secondary" onClick={fetchLogs} disabled={loading}>
          <RefreshCcw size={16} className={loading ? 'spin' : ''} />
          Refresh
        </button>
      </div>

      <div style={{ marginBottom: '20px' }}>
        <p className="text-sm text-muted-foreground" style={{ marginBottom: '8px' }}>
          <strong>Outbound Channels Disclosure:</strong>
        </p>
        <ul className="text-sm text-muted-foreground" style={{ listStyleType: 'disc', paddingLeft: '20px' }}>
          {OUTBOUND_CHANNEL_DISCLOSURE.map((item, i) => (
            <li key={i}>{item}</li>
          ))}
        </ul>
      </div>

      {error ? (
        <div style={{ color: '#ef4444', marginBottom: '16px' }}>{error}</div>
      ) : loading && logs.length === 0 ? (
        <div className="text-muted-foreground" style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
          <div className="spinner" style={{ width: 16, height: 16 }} />
          Loading network activity...
        </div>
      ) : logs.length === 0 ? (
        <div className="text-muted-foreground">No outbound requests recorded yet.</div>
      ) : (
        <div style={{ overflowX: 'auto' }}>
          <table style={{ width: '100%', textAlign: 'left', borderCollapse: 'collapse' }}>
            <thead>
              <tr style={{ borderBottom: '1px solid var(--border)', fontSize: '0.85rem', color: 'var(--text-muted)' }}>
                <th style={{ padding: '8px 4px' }}>Timestamp</th>
                <th style={{ padding: '8px 4px' }}>Method</th>
                <th style={{ padding: '8px 4px' }}>Domain</th>
                <th style={{ padding: '8px 4px' }}>URL (Redacted)</th>
                <th style={{ padding: '8px 4px' }}>Sent (B)</th>
                <th style={{ padding: '8px 4px' }}>Recv (B)</th>
                <th style={{ padding: '8px 4px' }}>Status</th>
              </tr>
            </thead>
            <tbody>
              {logs.map((log) => (
                <tr key={log.id} style={{ borderBottom: '1px solid var(--border-color)' }}>
                  <td style={{ padding: '8px 4px', fontSize: '0.85rem' }}>{new Date(log.timestamp).toLocaleString()}</td>
                  <td style={{ padding: '8px 4px', fontSize: '0.85rem' }}>{log.method}</td>
                  <td style={{ padding: '8px 4px', fontSize: '0.85rem' }}>{log.domain}</td>
                  <td style={{ padding: '8px 4px', fontSize: '0.85rem' }}>{log.url_redacted}</td>
                  <td style={{ padding: '8px 4px', fontSize: '0.85rem' }}>{log.bytes_sent ?? '-'}</td>
                  <td style={{ padding: '8px 4px', fontSize: '0.85rem' }}>{log.bytes_received ?? '-'}</td>
                  <td style={{ padding: '8px 4px', fontSize: '0.85rem' }}>{log.status_code ?? '-'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
