import { useNotificationCenter } from './notificationCenter/useNotificationCenter';
import TaskCard from './notificationCenter/TaskCard';
import {
  CenterHeader,
  CollapsedSummary,
  ExpandedFeed,
} from './notificationCenter/CenterPanels';

export default function SidebarNotificationCenter() {
  const center = useNotificationCenter();
  const { primaryTask, isExpanded } = center;

  if (!center.hasContent) return null;

  const showCollapsedSummary =
    !isExpanded && (center.activeTasks.length > 1 || center.visibleNotifications.length > 0);

  return (
    <div
      className="mx-3 mb-3 rounded-xl flex flex-col transition-all duration-300 overflow-hidden border border-[#F8E7C9]/15 shadow-sm"
      style={{ backgroundColor: 'rgba(248,231,201,0.06)', backdropFilter: 'blur(8px)' }}
      data-testid="sidebar-notification-center"
      // Legacy test id hooks for backward compatibility with existing test suites
      {...(center.hasScanTask ? { 'data-testid-scan': 'scan-status-sidebar-item' } : {})}
    >
      <CenterHeader center={center} />

      <div className="p-3 flex flex-col gap-3">
        {primaryTask && (
          <TaskCard
            task={primaryTask}
            onCancel={() => center.cancelTask(primaryTask.id)}
            onDismiss={() => center.removeTask(primaryTask.id)}
          />
        )}

        {showCollapsedSummary && <CollapsedSummary center={center} />}

        {isExpanded && <ExpandedFeed center={center} />}
      </div>
    </div>
  );
}
