import { useState, useMemo } from 'react';
import {
  Loader2,
  CheckCircle2,
  AlertCircle,
  XCircle,
  X,
  ChevronDown,
  ChevronUp,
  Sparkles,
  FileText,
  Activity,
  Database,
  Bell,
  ArrowUpRight,
} from 'lucide-react';
import { Link } from 'react-router-dom';
import {
  useNotificationStore,
  type UnifiedTask,
  type NotificationFeedItem,
  type NotificationCategory,
} from '@/stores/useNotificationStore';
import { formatDuration } from '@/lib/scanTiming';
import { cn } from '@/lib/utils';

function getCategoryIcon(category: NotificationCategory) {
  switch (category) {
    case 'ingestion':
      return Activity;
    case 'statements':
      return FileText;
    case 'normalization':
      return Sparkles;
    case 'database':
      return Database;
    case 'system':
    default:
      return Bell;
  }
}

function formatTimeAgo(timestamp: number): string {
  const diffSec = Math.max(0, Math.floor((Date.now() - timestamp) / 1000));
  if (diffSec < 10) return 'Just now';
  if (diffSec < 60) return `${diffSec}s ago`;
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHours = Math.floor(diffMin / 60);
  return `${diffHours}h ago`;
}

export default function SidebarNotificationCenter() {
  const tasksObj = useNotificationStore((s) => s.tasks);
  const notifications = useNotificationStore((s) => s.notifications);
  const isExpanded = useNotificationStore((s) => s.isExpanded);
  const toggleExpanded = useNotificationStore((s) => s.toggleExpanded);
  const cancelTask = useNotificationStore((s) => s.cancelTask);
  const dismissNotification = useNotificationStore((s) => s.dismissNotification);
  const removeTask = useNotificationStore((s) => s.removeTask);

  const [filterCategory, setFilterCategory] = useState<string | null>(null);

  const activeTasks = useMemo(
    () => Object.values(tasksObj).filter((t) => t.status === 'running' || t.status === 'cancelling'),
    [tasksObj]
  );

  const recentFinishedTasks = useMemo(
    () => Object.values(tasksObj).filter((t) => t.status !== 'running' && t.status !== 'cancelling'),
    [tasksObj]
  );

  const visibleNotifications = useMemo(() => {
    let list = notifications.filter((n) => !n.dismissed);
    if (filterCategory) {
      list = list.filter((n) => n.category === filterCategory);
    }
    return list;
  }, [notifications, filterCategory]);

  const hasContent = activeTasks.length > 0 || visibleNotifications.length > 0 || recentFinishedTasks.length > 0;

  if (!hasContent) return null;

  const primaryTask = activeTasks[0] ?? recentFinishedTasks[0];

  return (
    <div
      className="mx-3 mb-3 rounded-xl flex flex-col transition-all duration-300 overflow-hidden border border-[#F8E7C9]/15 shadow-sm"
      style={{
        backgroundColor: 'rgba(248,231,201,0.06)',
        backdropFilter: 'blur(8px)',
      }}
      data-testid="sidebar-notification-center"
      // Legacy test id hooks for backward compatibility with existing test suites
      {...(activeTasks.some((t) => t.id.startsWith('scan:')) || recentFinishedTasks.some((t) => t.id.startsWith('scan:'))
        ? { 'data-testid-scan': 'scan-status-sidebar-item' }
        : {})}
    >
      {/* Header bar */}
      <div className="px-3.5 py-2.5 flex items-center justify-between border-b border-[#F8E7C9]/10 select-none">
        <div className="flex items-center gap-2">
          {activeTasks.length > 0 ? (
            <div className="relative flex items-center justify-center">
              <span className="animate-ping absolute inline-flex h-2 w-2 rounded-full bg-[#10b981] opacity-75" />
              <span className="relative inline-flex rounded-full h-2 w-2 bg-[#10b981]" />
            </div>
          ) : (
            <Bell className="w-3.5 h-3.5 text-[#F8E7C9]/70" aria-hidden="true" />
          )}

          <span className="text-[12px] font-semibold tracking-wide text-[#F8E7C9]">
            {activeTasks.length > 0
              ? `${activeTasks.length} Active Process${activeTasks.length === 1 ? '' : 'es'}`
              : 'Notifications'}
          </span>
        </div>

        <button
          type="button"
          onClick={toggleExpanded}
          className="p-1 rounded-md text-[#F8E7C9]/60 hover:text-[#F8E7C9] hover:bg-[#F8E7C9]/10 transition-colors"
          aria-expanded={isExpanded}
          aria-label={isExpanded ? 'Collapse notifications' : 'Expand notifications'}
        >
          {isExpanded ? <ChevronDown className="w-3.5 h-3.5" /> : <ChevronUp className="w-3.5 h-3.5" />}
        </button>
      </div>

      {/* Main card body */}
      <div className="p-3 flex flex-col gap-3">
        {/* Active / Primary Task Item */}
        {primaryTask && (
          <TaskCard
            task={primaryTask}
            onCancel={() => cancelTask(primaryTask.id)}
            onDismiss={() => removeTask(primaryTask.id)}
          />
        )}

        {/* Compact Mode secondary summary if collapsed and more items exist */}
        {!isExpanded && (activeTasks.length > 1 || visibleNotifications.length > 0) && (
          <button
            type="button"
            onClick={toggleExpanded}
            className="text-[11px] font-medium text-[#F8E7C9]/60 hover:text-[#F8E7C9] text-left transition-colors flex items-center justify-between pt-1"
          >
            <span>
              +{activeTasks.length > 1 ? activeTasks.length - 1 : 0} task
              {activeTasks.length > 2 ? 's' : ''}
              {visibleNotifications.length > 0
                ? ` · ${visibleNotifications.length} notification${visibleNotifications.length === 1 ? '' : 's'}`
                : ''}
            </span>
            <ChevronUp className="w-3 h-3 text-[#F8E7C9]/40" />
          </button>
        )}

        {/* Expanded View: Additional active tasks & Notification Feed */}
        {isExpanded && (
          <div className="flex flex-col gap-3 pt-1 border-t border-[#F8E7C9]/10 mt-1 max-h-[240px] overflow-y-auto pr-1">
            {/* Additional Active Tasks */}
            {activeTasks.slice(1).map((t) => (
              <TaskCard
                key={t.id}
                task={t}
                onCancel={() => cancelTask(t.id)}
                onDismiss={() => removeTask(t.id)}
              />
            ))}

            {/* Notification Feed Items */}
            {visibleNotifications.length > 0 && (
              <div className="flex flex-col gap-2 pt-1">
                <div className="text-[10px] font-semibold uppercase tracking-wider text-[#F8E7C9]/40">
                  Recent Alerts
                </div>
                {visibleNotifications.map((item) => (
                  <NotificationCard
                    key={item.id}
                    item={item}
                    onDismiss={() => dismissNotification(item.id)}
                  />
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function TaskCard({
  task,
  onCancel,
  onDismiss,
}: {
  task: UnifiedTask;
  onCancel: () => void;
  onDismiss: () => void;
}) {
  const Icon = getCategoryIcon(task.category);
  const isRunning = task.status === 'running' || task.status === 'cancelling';

  return (
    <div className="flex flex-col gap-2 rounded-lg p-2.5 bg-[#064E3B]/40 border border-[#F8E7C9]/10">
      <div className="flex items-start justify-between gap-2">
        <div className="flex items-center gap-2 min-w-0">
          {isRunning ? (
            <Loader2 className="w-3.5 h-3.5 animate-spin shrink-0 text-[#F8E7C9]" aria-hidden="true" />
          ) : task.status === 'completed' ? (
            <CheckCircle2 className="w-3.5 h-3.5 shrink-0 text-[#10b981]" aria-hidden="true" />
          ) : task.status === 'cancelled' ? (
            <XCircle className="w-3.5 h-3.5 shrink-0 text-[#F8E7C9]/50" aria-hidden="true" />
          ) : (
            <AlertCircle className="w-3.5 h-3.5 shrink-0 text-[#ef4444]" aria-hidden="true" />
          )}

          <div className="flex flex-col min-w-0">
            <span className="text-[12.5px] font-semibold text-[#F8E7C9] leading-tight truncate">
              {task.title}
            </span>
            {task.description && (
              <span className="text-[11px] text-[#F8E7C9]/70 leading-normal truncate">
                {task.description}
              </span>
            )}
          </div>
        </div>

        <div className="flex items-center gap-1 shrink-0">
          {task.cancelable && isRunning && (
            <button
              type="button"
              onClick={onCancel}
              className="text-[10.5px] font-medium px-1.5 py-0.5 rounded bg-red-500/20 text-red-200 hover:bg-red-500/30 transition-colors"
              aria-label="Cancel pipeline process"
            >
              Cancel
            </button>
          )}

          {!isRunning && (
            <button
              type="button"
              onClick={onDismiss}
              className="p-1 text-[#F8E7C9]/40 hover:text-[#F8E7C9] rounded transition-colors"
              aria-label="Dismiss task notification"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          )}
        </div>
      </div>

      {/* Live Progress Bar */}
      {isRunning && (
        <div className="flex flex-col gap-1 mt-0.5">
          <div className="w-full h-1.5 rounded-full overflow-hidden bg-[#F8E7C9]/10">
            <div
              className="h-full rounded-full bg-[#F8E7C9] transition-all duration-300"
              style={{ width: `${task.progressPct}%` }}
            />
          </div>

          <div className="flex items-center justify-between text-[10px] text-[#F8E7C9]/60 font-medium">
            <span>
              {task.total > 0 ? `${task.current}/${task.total} (${task.progressPct}%)` : `${task.progressPct}%`}
            </span>
            {task.etaSeconds != null && task.etaSeconds > 0 && (
              <span>~{formatDuration(task.etaSeconds)} remaining</span>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function NotificationCard({
  item,
  onDismiss,
}: {
  item: NotificationFeedItem;
  onDismiss: () => void;
}) {
  const Icon = getCategoryIcon(item.category);

  const severityStyles =
    item.severity === 'error'
      ? 'border-red-500/30 bg-red-500/10 text-red-200'
      : item.severity === 'warning'
        ? 'border-amber-400/30 bg-amber-400/10 text-amber-200'
        : item.severity === 'success'
          ? 'border-emerald-500/30 bg-emerald-500/10 text-emerald-200'
          : 'border-[#F8E7C9]/10 bg-[#064E3B]/30 text-[#F8E7C9]';

  return (
    <div className={cn('flex flex-col gap-1 rounded-lg p-2 border text-[11.5px]', severityStyles)}>
      <div className="flex items-start justify-between gap-1.5">
        <div className="flex items-center gap-1.5 font-semibold leading-tight">
          <Icon className="w-3 h-3 shrink-0 opacity-80" />
          <span>{item.title}</span>
        </div>

        <div className="flex items-center gap-1 shrink-0">
          <span className="text-[9.5px] opacity-60 font-normal">{formatTimeAgo(item.timestamp)}</span>
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
