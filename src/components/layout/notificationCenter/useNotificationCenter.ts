/**
 * Selects and orders tasks and notifications for display.
 */
import { useMemo } from 'react';
import { useNotificationStore } from '@/stores/useNotificationStore';

/** Selects and orders tasks and notifications for display. */
export function useNotificationCenter() {
  const tasksObj = useNotificationStore((s) => s.tasks);
  const notifications = useNotificationStore((s) => s.notifications);
  const isExpanded = useNotificationStore((s) => s.isExpanded);
  const toggleExpanded = useNotificationStore((s) => s.toggleExpanded);
  const cancelTask = useNotificationStore((s) => s.cancelTask);
  const dismissNotification = useNotificationStore((s) => s.dismissNotification);
  const removeTask = useNotificationStore((s) => s.removeTask);

  const activeTasks = useMemo(
    () => Object.values(tasksObj).filter((t) => t.status === 'running' || t.status === 'cancelling'),
    [tasksObj]
  );

  const recentFinishedTasks = useMemo(
    () => Object.values(tasksObj).filter((t) => t.status !== 'running' && t.status !== 'cancelling'),
    [tasksObj]
  );

  const visibleNotifications = useMemo(
    () => notifications.filter((n) => !n.dismissed),
    [notifications]
  );

  return {
    activeTasks,
    recentFinishedTasks,
    visibleNotifications,
    isExpanded,
    toggleExpanded,
    cancelTask,
    dismissNotification,
    removeTask,
    hasContent:
      activeTasks.length > 0 ||
      visibleNotifications.length > 0 ||
      recentFinishedTasks.length > 0,
    primaryTask: activeTasks[0] ?? recentFinishedTasks[0],
    hasScanTask:
      activeTasks.some((t) => t.id.startsWith('scan:')) ||
      recentFinishedTasks.some((t) => t.id.startsWith('scan:')),
  };
}
