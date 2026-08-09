import { Bell, ChevronDown, ChevronUp } from 'lucide-react';
import TaskCard from './TaskCard';
import NotificationCard from './NotificationCard';
import type { useNotificationCenter } from './useNotificationCenter';

type Center = ReturnType<typeof useNotificationCenter>;

export function CenterHeader({ center }: { center: Center }) {
  const active = center.activeTasks.length;

  return (
    <div className="px-3.5 py-2.5 flex items-center justify-between border-b border-[#F8E7C9]/10 select-none">
      <div className="flex items-center gap-2">
        {active > 0 ? (
          <div className="relative flex items-center justify-center">
            <span className="animate-ping absolute inline-flex h-2 w-2 rounded-full bg-[#10b981] opacity-75" />
            <span className="relative inline-flex rounded-full h-2 w-2 bg-[#10b981]" />
          </div>
        ) : (
          <Bell className="w-3.5 h-3.5 text-[#F8E7C9]/70" aria-hidden="true" />
        )}

        <span className="text-[12px] font-semibold tracking-wide text-[#F8E7C9]">
          {active > 0 ? `${active} Active Process${active === 1 ? '' : 'es'}` : 'Notifications'}
        </span>
      </div>

      <button
        type="button"
        onClick={center.toggleExpanded}
        className="p-1 rounded-md text-[#F8E7C9]/60 hover:text-[#F8E7C9] hover:bg-[#F8E7C9]/10 transition-colors"
        aria-expanded={center.isExpanded}
        aria-label={center.isExpanded ? 'Collapse notifications' : 'Expand notifications'}
      >
        {center.isExpanded ? (
          <ChevronDown className="w-3.5 h-3.5" />
        ) : (
          <ChevronUp className="w-3.5 h-3.5" />
        )}
      </button>
    </div>
  );
}

/** Collapsed summary of everything the single primary card isn't showing. */
export function CollapsedSummary({ center }: { center: Center }) {
  const extraTasks = center.activeTasks.length > 1 ? center.activeTasks.length - 1 : 0;
  const alerts = center.visibleNotifications.length;

  return (
    <button
      type="button"
      onClick={center.toggleExpanded}
      className="text-[11px] font-medium text-[#F8E7C9]/60 hover:text-[#F8E7C9] text-left transition-colors flex items-center justify-between pt-1"
    >
      <span>
        +{extraTasks} task{center.activeTasks.length > 2 ? 's' : ''}
        {alerts > 0 ? ` · ${alerts} notification${alerts === 1 ? '' : 's'}` : ''}
      </span>
      <ChevronUp className="w-3 h-3 text-[#F8E7C9]/40" />
    </button>
  );
}

export function ExpandedFeed({ center }: { center: Center }) {
  return (
    <div className="flex flex-col gap-3 pt-1 border-t border-[#F8E7C9]/10 mt-1 max-h-[240px] overflow-y-auto pr-1">
      {center.activeTasks.slice(1).map((t) => (
        <TaskCard
          key={t.id}
          task={t}
          onCancel={() => center.cancelTask(t.id)}
          onDismiss={() => center.removeTask(t.id)}
        />
      ))}

      {center.visibleNotifications.length > 0 && (
        <div className="flex flex-col gap-2 pt-1">
          <div className="text-[10px] font-semibold uppercase tracking-wider text-[#F8E7C9]/40">
            Recent Alerts
          </div>
          {center.visibleNotifications.map((item) => (
            <NotificationCard
              key={item.id}
              item={item}
              onDismiss={() => center.dismissNotification(item.id)}
            />
          ))}
        </div>
      )}
    </div>
  );
}
