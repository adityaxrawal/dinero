/**
 * Warns when Gmail is disconnected and ingestion has therefore stopped.
 */
import { useEffect, useState } from 'react';
import { AlertTriangle, Info, WifiOff } from 'lucide-react';
import { API, type SystemWarningPayload } from '@/lib/ipc';
import { useIpcListen } from '@/hooks/useIpcListen';

const OWNED_ELSEWHERE = new Set(['keychain_denied', 'notification_denied']);

const SEVERITY_RANK: Record<SystemWarningPayload['severity'], number> = {
  critical: 3,
  degraded: 2,
  info: 1,
};

/** Picks the most severe warning when several are active. */
function highestPriority(warnings: SystemWarningPayload[]): SystemWarningPayload | null {
  let best: SystemWarningPayload | null = null;
  for (const w of warnings) {
    if (!best || SEVERITY_RANK[w.severity] > SEVERITY_RANK[best.severity]) {
      best = w;
    }
  }
  return best;
}

/** Warns when Gmail is disconnected and ingestion has stopped. */
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
