import { useEffect, useState } from 'react';
import { Loader2, ChevronDown, ChevronUp, AlertCircle, X } from 'lucide-react';
import { API, type BackgroundTaskProgressPayload } from '@/lib/ipc';
import { useIpcListen } from '@/hooks/useIpcListen';

type TaskProgress = BackgroundTaskProgressPayload;

/**
 * TASK-DESK-003/TASK-RT-004 (Doc 30 §12): persistent, non-blocking
 * background task indicator. Redesigned as a floating pill in the
 * bottom-right corner of the main content area. Renders nothing when there
 * are no active or failed tasks to show.
 *
 * TASK-RT-004 fix: a failed task previously vanished from state the
 * instant its terminal `background_task_progress` event arrived (the same
 * branch handled `completed` and `failed` identically) -- a real scan
 * failure was invisible, indistinguishable from success. Failed tasks now
 * persist as a distinct error chip until the user dismisses it; only
 * `completed` tasks are removed automatically.
 *
 * TASK-RT-004: fetches `get_active_background_tasks` on mount (late-mount
 * recovery) so a component that mounts (or remounts across a route
 * navigation) after a task already started still shows it, rather than
 * only ever reacting to the live event.
 */
export default function BackgroundTaskIndicator() {
  const [tasks, setTasks] = useState<Record<string, TaskProgress>>({});
  const [expanded, setExpanded] = useState(false);
  const [detailsFor, setDetailsFor] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    API.backgroundTasks
      .getActive()
      .then((active) => {
        if (cancelled) return;
        setTasks((prev) => {
          const next = { ...prev };
          for (const task of active) {
            next[task.task_id] = task;
          }
          return next;
        });
      })
      .catch((e) => console.error('Failed to fetch active background tasks', e));
    return () => {
      cancelled = true;
    };
  }, []);

  useIpcListen<TaskProgress>('background_task_progress', (progress) => {
    setTasks((prev) => {
      if (progress.status === 'completed') {
        const next = { ...prev };
        delete next[progress.task_id];
        return next;
      }
      // 'running' and 'failed' both persist -- a failed task stays visible
      // as an error chip until the user dismisses it.
      return { ...prev, [progress.task_id]: progress };
    });
  });

  const dismissTask = (taskId: string) => {
    setTasks((prev) => {
      const next = { ...prev };
      delete next[taskId];
      return next;
    });
    setDetailsFor((d) => (d === taskId ? null : d));
  };

  const all = Object.values(tasks);
  if (all.length === 0) return null;

  const running = all.filter((t) => t.status === 'running');
  const failed = all.filter((t) => t.status === 'failed');

  const summary =
    running.length === 0
      ? `${failed.length} task${failed.length === 1 ? '' : 's'} failed`
      : running.length === 1
        ? running[0].label
        : `${running.length} tasks running`;

  return (
    <div
      className="overflow-hidden rounded-xl shadow-lg"
      style={{
        background: 'hsl(38, 55%, 91%)',
        border: '1px solid #d9c8a8',
        maxWidth: '280px',
        minWidth: '200px',
      }}
      role="status"
      aria-live="polite"
      data-testid="bg-task-indicator"
    >
      <button
        type="button"
        className="w-full px-3 py-2.5 flex items-center gap-2.5 text-left disabled:cursor-default"
        onClick={() => setExpanded((e) => !e)}
        aria-expanded={expanded}
        aria-label={summary}
        disabled={all.length <= 1}
      >
        {running.length > 0 ? (
          <Loader2
            className="w-3.5 h-3.5 animate-spin shrink-0"
            style={{ color: '#064E3B' }}
            aria-hidden="true"
          />
        ) : (
          <AlertCircle className="w-3.5 h-3.5 shrink-0 text-red-500" aria-hidden="true" />
        )}
        <div className="flex-1 min-w-0 text-xs font-medium truncate" style={{ color: '#3d5a50' }}>
          {summary}
        </div>
        {all.length > 1 && (
          expanded
            ? <ChevronUp className="w-3.5 h-3.5 shrink-0" style={{ color: '#6b8a7f' }} aria-hidden="true" />
            : <ChevronDown className="w-3.5 h-3.5 shrink-0" style={{ color: '#6b8a7f' }} aria-hidden="true" />
        )}
      </button>

      {/* Single task: progress bar (running) or error chip (failed) */}
      {all.length === 1 && running.length === 1 && running[0].total > 0 && (
        <div className="px-3 pb-2.5">
          <div className="w-full h-1 rounded-full overflow-hidden" style={{ background: 'rgba(6,78,59,0.10)' }}>
            <div
              className="h-full rounded-full transition-all duration-300"
              style={{ width: `${running[0].progress_pct}%`, background: '#064E3B' }}
            />
          </div>
          <div className="mt-1 text-[10px]" style={{ color: '#6b8a7f' }}>
            {running[0].current}/{running[0].total}
            {running[0].eta_seconds != null && ` · ~${running[0].eta_seconds}s`}
          </div>
        </div>
      )}
      {all.length === 1 && failed.length === 1 && (
        <FailedTaskChip
          task={failed[0]}
          showingDetails={detailsFor === failed[0].task_id}
          onToggleDetails={() =>
            setDetailsFor((d) => (d === failed[0].task_id ? null : failed[0].task_id))
          }
          onDismiss={() => dismissTask(failed[0].task_id)}
        />
      )}

      {/* Expanded multi-task list */}
      {expanded && all.length > 1 && (
        <div style={{ borderTop: '1px solid #d9c8a8' }}>
          {all.map((task) =>
            task.status === 'failed' ? (
              <FailedTaskChip
                key={task.task_id}
                task={task}
                showingDetails={detailsFor === task.task_id}
                onToggleDetails={() =>
                  setDetailsFor((d) => (d === task.task_id ? null : task.task_id))
                }
                onDismiss={() => dismissTask(task.task_id)}
              />
            ) : (
              <div key={task.task_id} className="px-3 py-2" style={{ borderBottom: '1px solid rgba(217,200,168,0.40)' }}>
                <div className="text-xs truncate" style={{ color: '#3d5a50' }}>{task.label}</div>
                {task.total > 0 && (
                  <>
                    <div
                      className="mt-1 w-full h-1 rounded-full overflow-hidden"
                      style={{ background: 'rgba(6,78,59,0.10)' }}
                    >
                      <div
                        className="h-full rounded-full"
                        style={{ width: `${task.progress_pct}%`, background: '#064E3B' }}
                      />
                    </div>
                    <div className="mt-0.5 text-[10px]" style={{ color: '#6b8a7f' }}>
                      {task.current}/{task.total} ({task.progress_pct.toFixed(0)}%)
                      {task.eta_seconds != null && ` · ~${task.eta_seconds}s`}
                    </div>
                  </>
                )}
              </div>
            ),
          )}
        </div>
      )}
    </div>
  );
}

function FailedTaskChip({
  task,
  showingDetails,
  onToggleDetails,
  onDismiss,
}: {
  task: TaskProgress;
  showingDetails: boolean;
  onToggleDetails: () => void;
  onDismiss: () => void;
}) {
  return (
    <div
      data-testid={`failed-task-chip-${task.task_id}`}
      className="px-3 py-2 flex flex-col gap-1"
      style={{ borderBottom: '1px solid rgba(217,200,168,0.40)' }}
    >
      <div className="flex items-center gap-2">
        <AlertCircle className="w-3 h-3 shrink-0 text-red-500" aria-hidden="true" />
        <div className="flex-1 min-w-0 text-xs truncate" style={{ color: '#8a2b2b' }}>
          {task.label}
        </div>
        <button
          type="button"
          onClick={onToggleDetails}
          className="text-[10px] font-semibold shrink-0 underline"
          style={{ color: '#8a2b2b' }}
        >
          View Details
        </button>
        <button
          type="button"
          onClick={onDismiss}
          aria-label="Dismiss failed task"
          className="shrink-0"
        >
          <X className="w-3 h-3" style={{ color: '#8a2b2b' }} aria-hidden="true" />
        </button>
      </div>
      {showingDetails && (
        <div className="text-[10.5px] pl-5" style={{ color: '#8a2b2b' }}>
          {task.status_message}
        </div>
      )}
    </div>
  );
}
