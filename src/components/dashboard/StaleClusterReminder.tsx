/**
 * Reminds the user about reconciliation clusters left unresolved.
 *
 * Dismissal is local state only, so it reappears next launch rather than being
 * permanently suppressed by one click. Not a modal -- a stale cluster is
 * low-urgency backlog, not something demanding immediate action.
 */
import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Clock, X } from 'lucide-react';
import type { ClusterRecord } from '@/lib/ipc';

import { isClusterStale } from './isClusterStale';

interface StaleClusterReminderProps {
  clusters: ClusterRecord[];
}

/**
 * Reminds the user about clusters left unresolved.
 *
 * Dismissal is local state only, so it reappears next launch rather than being
 * permanently suppressed by one click.
 */
export default function StaleClusterReminder({ clusters }: StaleClusterReminderProps) {
  const navigate = useNavigate();
  const [dismissed, setDismissed] = useState(false);

  const staleCount = clusters.filter((c) => isClusterStale(c.created_at)).length;
  if (staleCount === 0 || dismissed) return null;

  return (
    <div
      role="status"
      data-testid="stale-cluster-reminder"
      className="flex items-center gap-3 px-4 py-3 rounded-xl border card-champagne"
      style={{ borderColor: 'rgba(6,78,59,0.15)' }}
    >
      <Clock className="w-4 h-4 shrink-0" style={{ color: '#6b8a7f' }} aria-hidden="true" />
      <p className="flex-1 text-xs" style={{ color: 'var(--text-secondary)' }}>
        {staleCount} transaction match{staleCount === 1 ? '' : 'es'} still need review — open more
        than 7 days.
      </p>
      <button
        type="button"
        className="text-xs font-medium hover:underline shrink-0"
        style={{ color: '#064E3B' }}
        onClick={() => navigate('/reconciliation')}
      >
        Review now
      </button>
      <button
        type="button"
        onClick={() => setDismissed(true)}
        aria-label="Dismiss stale cluster reminder"
        className="shrink-0"
        style={{ color: 'var(--text-muted)' }}
      >
        <X className="w-3.5 h-3.5" aria-hidden="true" />
      </button>
    </div>
  );
}
