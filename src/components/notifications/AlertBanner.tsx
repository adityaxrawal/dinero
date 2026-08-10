/**
 * Shows the highest-priority active budget alert.
 */
import { AlertTriangle, X } from 'lucide-react';
import { useAlertStore, highestPriorityAlert } from '@/stores/useAlertStore';

/** Shows the highest-priority active budget alert. */
export default function AlertBanner() {
  const alerts = useAlertStore((s) => s.alerts);
  const dismissed = useAlertStore((s) => s.dismissed);
  const dismissAlert = useAlertStore((s) => s.dismissAlert);

  const visible = alerts.filter((a) => !dismissed.has(a.alert_type));
  const top = highestPriorityAlert(visible);
  if (!top) return null;

  const isExhausted = top.alert_type.endsWith('_100');
  const palette = isExhausted
    ? {
        border: 'border-red-500/40',
        bg: 'bg-red-500/10',
        icon: 'text-red-400',
        text: 'text-red-200',
      }
    : {
        border: 'border-amber-400/30',
        bg: 'bg-amber-400/10',
        icon: 'text-amber-300',
        text: 'text-amber-200',
      };

  return (
    <div
      role="status"
      data-testid="alert-banner"
      data-alert-type={top.alert_type}
      className={`flex items-start gap-2 mx-4 mb-2 px-3 py-2.5 rounded-lg border ${palette.border} ${palette.bg}`}
    >
      <AlertTriangle className={`w-3.5 h-3.5 ${palette.icon} shrink-0 mt-0.5`} aria-hidden="true" />
      <p className={`flex-1 text-[11.5px] leading-snug ${palette.text}`}>{top.message}</p>
      <button
        onClick={() => dismissAlert(top.alert_type)}
        aria-label="Dismiss spending alert"
        className={`${isExhausted ? 'text-red-400/50 hover:text-red-200' : 'text-amber-300/50 hover:text-amber-200'} shrink-0`}
      >
        <X className="w-3.5 h-3.5" aria-hidden="true" />
      </button>
    </div>
  );
}
