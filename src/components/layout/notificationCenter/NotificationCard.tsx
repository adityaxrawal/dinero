import { Link } from 'react-router-dom';
import { X, Sparkles, FileText, Activity, Database, Bell, ArrowUpRight } from 'lucide-react';
import type {
  NotificationFeedItem,
  NotificationCategory,
} from '@/stores/useNotificationStore';
import { cn } from '@/lib/utils';

const CATEGORY_ICON: Record<NotificationCategory, typeof Bell> = {
  ingestion: Activity,
  statements: FileText,
  normalization: Sparkles,
  database: Database,
  system: Bell,
};

const SEVERITY_STYLES: Record<string, string> = {
  error: 'border-red-500/30 bg-red-500/10 text-red-200',
  warning: 'border-amber-400/30 bg-amber-400/10 text-amber-200',
  success: 'border-emerald-500/30 bg-emerald-500/10 text-emerald-200',
};

const DEFAULT_SEVERITY = 'border-[#F8E7C9]/10 bg-[#064E3B]/30 text-[#F8E7C9]';

function formatTimeAgo(timestamp: number): string {
  const diffSec = Math.max(0, Math.floor((Date.now() - timestamp) / 1000));
  if (diffSec < 10) return 'Just now';
  if (diffSec < 60) return `${diffSec}s ago`;
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin}m ago`;
  return `${Math.floor(diffMin / 60)}h ago`;
}

export default function NotificationCard({
  item,
  onDismiss,
}: {
  item: NotificationFeedItem;
  onDismiss: () => void;
}) {
  // Not a component created during render: the lookup returns one of five
  // module-level lucide components, so the identity is stable per category and
  // nothing remounts.
  const Icon = CATEGORY_ICON[item.category] ?? Bell;

  return (
    <div
      className={cn(
        'flex flex-col gap-1 rounded-lg p-2 border text-[11.5px]',
        SEVERITY_STYLES[item.severity] ?? DEFAULT_SEVERITY
      )}
    >
      <div className="flex items-start justify-between gap-1.5">
        <div className="flex items-center gap-1.5 font-semibold leading-tight">
          <Icon className="w-3 h-3 shrink-0 opacity-80" />
          <span>{item.title}</span>
        </div>

        <div className="flex items-center gap-1 shrink-0">
          <span className="text-[9.5px] opacity-60 font-normal">
            {formatTimeAgo(item.timestamp)}
          </span>
          <button
            type="button"
            onClick={onDismiss}
            className="p-0.5 opacity-50 hover:opacity-100 rounded"
            aria-label="Dismiss alert"
          >
            <X className="w-3 h-3" />
          </button>
        </div>
      </div>

      <p className="opacity-90 leading-snug text-[11px]">{item.message}</p>

      {item.actionUrl && item.actionLabel && (
        <Link
          to={item.actionUrl}
          className="mt-1 inline-flex items-center gap-1 text-[10.5px] font-semibold underline underline-offset-2 opacity-90 hover:opacity-100"
        >
          {item.actionLabel}
          <ArrowUpRight className="w-3 h-3" />
        </Link>
      )}
    </div>
  );
}
