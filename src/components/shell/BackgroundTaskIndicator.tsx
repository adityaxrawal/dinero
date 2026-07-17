import { useEffect, useState } from 'react';
import { Loader2, ChevronDown, ChevronUp } from 'lucide-react';

interface TaskProgress {
  task_id: string;
  task_type: string;
  label: string;
  current: number;
  total: number;
  eta_seconds: number | null;
  status: 'running' | 'completed' | 'failed';
  progress_pct: number;
  status_message: string;
}

/**
 * TASK-DESK-003 (Doc 30 §12): a persistent, non-blocking indicator
 * aggregating every active long-running background task (historical scans,
 * and anything else that registers with the Rust-side
 * `BackgroundTaskRegistry`) -- a one-line summary when idle or a single
 * task is running, expandable to per-task detail once more than one runs
 * concurrently. Renders nothing when no task is active, and never blocks
 * interaction with the rest of the app (it's an inline sidebar element,
 * never a modal).
 *
 * Listens for the single Document 19 §15-catalogued `background_task_progress`
 * event -- a task is considered finished when its `status` stops being
 * `"running"`, not by inferring completion from `current === total` (which
 * would be wrong for a task that fails or is cancelled partway through).
 */
export default function BackgroundTaskIndicator() {
  const [tasks, setTasks] = useState<Record<string, TaskProgress>>({});
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    const setup = async () => {
      let listen;
      try {
        const m = await import('@tauri-apps/api/event');
        listen = m.listen;
      } catch {
        return;
      }
      const handle = await listen<TaskProgress>('background_task_progress', (event) => {
        const progress = event.payload;
        setTasks((prev) => {
          if (progress.status !== 'running') {
            const next = { ...prev };
            delete next[progress.task_id];
            return next;
          }
          return { ...prev, [progress.task_id]: progress };
        });
      });
      unlisten = handle;
    };

    setup().catch((e) => console.error('Failed to listen for background_task_progress', e));
    return () => unlisten?.();
  }, []);

  const active = Object.values(tasks);
  if (active.length === 0) return null;

  const summary =
    active.length === 1 ? active[0].label : `${active.length} background tasks running`;

  return (
    <div
      className="mb-4 rounded-md bg-secondary border border-border overflow-hidden"
      role="status"
      aria-live="polite"
      data-testid="bg-task-indicator"
    >
      <button
        type="button"
        className="w-full p-3 flex items-center gap-3 text-left disabled:cursor-default"
        onClick={() => setExpanded((e) => !e)}
        aria-expanded={expanded}
        aria-label={summary}
        disabled={active.length <= 1}
      >
        <Loader2 className="w-4 h-4 animate-spin text-muted-foreground shrink-0" aria-hidden="true" />
        <div className="flex-1 min-w-0 text-xs text-muted-foreground truncate">{summary}</div>
        {active.length > 1 &&
          (expanded ? (
            <ChevronUp className="w-4 h-4 shrink-0" aria-hidden="true" />
          ) : (
            <ChevronDown className="w-4 h-4 shrink-0" aria-hidden="true" />
          ))}
      </button>
      {expanded && active.length > 1 && (
        <div className="border-t border-border divide-y divide-border">
          {active.map((task) => (
            <div key={task.task_id} className="px-3 py-2 text-xs text-muted-foreground">
              <div className="truncate">{task.label}</div>
              {task.total > 0 && (
                <div className="mt-1 text-[10px]">
                  {task.current}/{task.total} ({task.progress_pct.toFixed(0)}%)
                  {task.eta_seconds != null && ` · ~${task.eta_seconds}s remaining`}
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
