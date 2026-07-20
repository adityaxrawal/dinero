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
 * TASK-DESK-003 (Doc 30 §12): persistent, non-blocking background task indicator.
 * Redesigned as a floating pill in the bottom-right corner of the main content area.
 * Renders nothing when no tasks are active.
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
    active.length === 1 ? active[0].label : `${active.length} tasks running`;

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
        disabled={active.length <= 1}
      >
        <Loader2
          className="w-3.5 h-3.5 animate-spin shrink-0"
          style={{ color: '#064E3B' }}
          aria-hidden="true"
        />
        <div className="flex-1 min-w-0 text-xs font-medium truncate" style={{ color: '#3d5a50' }}>
          {summary}
        </div>
        {active.length > 1 && (
          expanded
            ? <ChevronUp className="w-3.5 h-3.5 shrink-0" style={{ color: '#6b8a7f' }} aria-hidden="true" />
            : <ChevronDown className="w-3.5 h-3.5 shrink-0" style={{ color: '#6b8a7f' }} aria-hidden="true" />
        )}
      </button>

      {/* Progress bar for single task */}
      {active.length === 1 && active[0].total > 0 && (
        <div className="px-3 pb-2.5">
          <div className="w-full h-1 rounded-full overflow-hidden" style={{ background: 'rgba(6,78,59,0.10)' }}>
            <div
              className="h-full rounded-full transition-all duration-300"
              style={{ width: `${active[0].progress_pct}%`, background: '#064E3B' }}
            />
          </div>
          <div className="mt-1 text-[10px]" style={{ color: '#6b8a7f' }}>
            {active[0].current}/{active[0].total}
            {active[0].eta_seconds != null && ` · ~${active[0].eta_seconds}s`}
          </div>
        </div>
      )}

      {/* Expanded multi-task list */}
      {expanded && active.length > 1 && (
        <div style={{ borderTop: '1px solid #d9c8a8' }}>
          {active.map((task) => (
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
          ))}
        </div>
      )}
    </div>
  );
}
