import { useEffect, useState } from 'react';
import { AlertTriangle, Info, WifiOff } from 'lucide-react';
import { API, type SystemWarningPayload } from '@/lib/ipc';
import { useIpcListen } from '@/hooks/useIpcListen';

/**
 * TASK-RT-007 (Doc 30, Doc 19 §15.1): renders the single highest-priority
 * active `system_warning` (low_ram, gmail_token_degraded, gmail_quota_exhausted,
 * clock_skew, ...) as a sidebar banner, same in-flow "System messages" slot as
 * `GracePeriodBanner`/`StatementOnlyModeBanner` (`AppLayout.tsx`).
 *
 * Two data sources, matching the Rust `ipc::system_warnings` module exactly:
 * - `get_active_system_warnings` on mount -- late-mount recovery, so a
 *   warning emitted before this component existed (e.g. low RAM detected at
 *   cold start) is still shown.
 * - Live `system_warning` / `system_warning_cleared` events thereafter.
 *
 * Deliberately excludes `keychain_denied`/`notification_denied`: those are
 * TASK-DESK-004's own warning family (`hard_fail`/`soft_fail` severity, not
 * this module's `critical`/`degraded`/`info`) with dedicated overlay
 * treatment in `PermissionDeniedOverlay.tsx` -- rendering them here too would
 * duplicate that UI.
 */
const OWNED_ELSEWHERE = new Set(['keychain_denied', 'notification_denied']);

const SEVERITY_RANK: Record<SystemWarningPayload['severity'], number> = {
  critical: 3,
  degraded: 2,
  info: 1,
};

function highestPriority(warnings: SystemWarningPayload[]): SystemWarningPayload | null {
  let best: SystemWarningPayload | null = null;
  for (const w of warnings) {
    if (!best || SEVERITY_RANK[w.severity] > SEVERITY_RANK[best.severity]) {
      best = w;
    }
  }
  return best;
}

export default function ConnectionStatusBanner() {
  const [warnings, setWarnings] = useState<SystemWarningPayload[]>([]);

  useEffect(() => {
    let cancelled = false;
    API.systemWarnings
      .getActive()
      .then((active) => {
        if (!cancelled) {
          setWarnings(active.filter((w) => !OWNED_ELSEWHERE.has(w.warning_type)));
        }
      })
      .catch((e) => console.error('Failed to fetch active system warnings', e));
    return () => {
      cancelled = true;
    };
  }, []);

  useIpcListen<SystemWarningPayload>('system_warning', (payload) => {
    if (OWNED_ELSEWHERE.has(payload.warning_type)) return;
    setWarnings((prev) => [
      ...prev.filter((w) => w.warning_type !== payload.warning_type),
      payload,
    ]);
  });

  useIpcListen<string>('system_warning_cleared', (warningType) => {
    setWarnings((prev) => prev.filter((w) => w.warning_type !== warningType));
  });

  const top = highestPriority(warnings);
  if (!top) return null;

  const palette =
    top.severity === 'critical'
      ? {
          border: 'border-red-500/40',
          bg: 'bg-red-500/10',
          icon: 'text-red-400',
          text: 'text-red-200',
        }
      : top.severity === 'degraded'
        ? {
            border: 'border-amber-400/30',
            bg: 'bg-amber-400/10',
            icon: 'text-amber-300',
            text: 'text-amber-200',
          }
        : {
            border: 'border-border',
            bg: 'bg-secondary',
            icon: 'text-muted-foreground',
            text: 'text-muted-foreground',
          };

  const Icon =
    top.severity === 'critical' ? WifiOff : top.severity === 'degraded' ? AlertTriangle : Info;

  return (
    <div
      role="status"
      data-testid="connection-status-banner"
      data-warning-type={top.warning_type}
      className={`flex items-start gap-2 mx-4 mb-2 px-3 py-2.5 rounded-lg border ${palette.border} ${palette.bg}`}
    >
      <Icon className={`w-3.5 h-3.5 ${palette.icon} shrink-0 mt-0.5`} aria-hidden="true" />
      <p className={`flex-1 text-[11.5px] leading-snug ${palette.text}`}>{top.message}</p>
    </div>
  );
}
