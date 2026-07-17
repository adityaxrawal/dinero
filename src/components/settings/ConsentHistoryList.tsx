import { Loader2, RefreshCw } from 'lucide-react';
import type { ConsentEventRecord } from '@/lib/ipc';

/**
 * TASK-FE-014 (Doc 30): "renders all consent events, always accessible,
 * showing granted/withdrawn timestamps." Extracted verbatim from the
 * pre-existing inline Settings.tsx section (Doc 25 §4.4) -- the loading
 * logic was already correct, this is a structural extraction only.
 */
export default function ConsentHistoryList({
  events,
  isLoading,
  onRefresh,
}: {
  events: ConsentEventRecord[];
  isLoading: boolean;
  onRefresh: () => void;
}) {
  return (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: '12px' }}>
        <p className="text-sm text-muted" style={{ margin: 0 }}>
          A record of what you've consented to and when — Gmail authorization, onboarding disclosures, and diagnostic bundle exports.
        </p>
        <button className="btn btn-secondary" onClick={onRefresh} disabled={isLoading} style={{ padding: '6px 12px', fontSize: '12px', flexShrink: 0, marginLeft: '12px' }} aria-label="Refresh consent history">
          {isLoading ? <Loader2 size={14} className="animate-spin" /> : <RefreshCw size={14} />}
        </button>
      </div>

      {isLoading ? (
        <p style={{ fontSize: '13px', color: 'var(--text-muted)' }}>Loading…</p>
      ) : events.length === 0 ? (
        <p style={{ fontSize: '13px', color: 'var(--text-muted)' }}>No consent events recorded yet.</p>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: '8px', maxHeight: '280px', overflowY: 'auto' }}>
          {events.map((event) => (
            <div
              key={event.id}
              style={{
                padding: '10px 14px',
                borderRadius: '8px',
                background: 'var(--bg-secondary)',
                border: '1px solid var(--border)',
                fontSize: '12px',
              }}
            >
              <div style={{ display: 'flex', justifyContent: 'space-between', gap: '12px' }}>
                <strong style={{ color: 'var(--text-primary)' }}>{event.event_type}</strong>
                <span style={{ color: 'var(--text-muted)', whiteSpace: 'nowrap' }}>
                  {new Date(event.consented_at).toLocaleString()}
                </span>
              </div>
              <p style={{ color: 'var(--text-muted)', marginTop: '4px' }}>{event.disclosure_text}</p>
              {event.withdrawn_at && (
                <p style={{ color: 'var(--text-muted)', marginTop: '4px', fontStyle: 'italic' }}>
                  Withdrawn {new Date(event.withdrawn_at).toLocaleString()}
                </p>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
