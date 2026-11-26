/**
 * One live background task, with progress and cancel affordance.
 */
import { Loader2, CheckCircle2, AlertCircle, XCircle, X } from 'lucide-react';
import type { UnifiedTask } from '@/stores/useNotificationStore';
import { formatDuration } from '@/lib/scanTiming';

const STATUS_ICON: Record<string, React.ReactNode> = {
  completed: <CheckCircle2 className="w-3.5 h-3.5 shrink-0 text-[#10b981]" aria-hidden="true" />,
  cancelled: <XCircle className="w-3.5 h-3.5 shrink-0 text-champagne/50" aria-hidden="true" />,
};

const RUNNING_ICON = (
  <Loader2 className="w-3.5 h-3.5 animate-spin shrink-0 text-champagne" aria-hidden="true" />
);
const FAILED_ICON = (
  <AlertCircle className="w-3.5 h-3.5 shrink-0 text-[#ef4444]" aria-hidden="true" />
);

/** Progress bar, indeterminate until a total is known. */
function ProgressBar({ task }: { task: UnifiedTask }) {
  const counts =
    task.total > 0
      ? `${task.current}/${task.total} (${task.progressPct}%)`
      : `${task.progressPct}%`;

  return (
    <div className="flex flex-col gap-1 mt-0.5">
      <div className="w-full h-1.5 rounded-full overflow-hidden bg-champagne/10">
        <div
          className="h-full rounded-full bg-champagne transition-all duration-300"
          style={{ width: `${task.progressPct}%` }}
        />
      </div>
      <div className="flex items-center justify-between text-[11px] text-champagne/60 font-medium">
        <span>{counts}</span>
        {task.etaSeconds != null && task.etaSeconds > 0 && (
          <span>~{formatDuration(task.etaSeconds)} remaining</span>
        )}
      </div>
    </div>
  );
}

/** One live background task, with progress and cancel. */
export default function TaskCard({
  task,
  onCancel,
  onDismiss,
}: {
  task: UnifiedTask;
  onCancel: () => void;
  onDismiss: () => void;
}) {
  const isRunning = task.status === 'running' || task.status === 'cancelling';

  return (
    <div className="flex flex-col gap-2 rounded-lg p-2.5 bg-emerald-ink/40 border border-champagne/10">
      <div className="flex items-start justify-between gap-2">
        <div className="flex items-center gap-2 min-w-0">
          {isRunning ? RUNNING_ICON : (STATUS_ICON[task.status] ?? FAILED_ICON)}

          <div className="flex flex-col min-w-0">
            <span className="text-[12.5px] font-semibold text-champagne leading-tight truncate">
              {task.title}
            </span>
            {task.description && (
              <span className="text-[11px] text-champagne/70 leading-normal truncate">
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
              className="text-[11px] font-medium px-1.5 py-0.5 rounded bg-red-500/20 text-red-200 hover:bg-red-500/30 transition-colors"
              aria-label="Cancel pipeline process"
            >
              Cancel
            </button>
          )}

          {!isRunning && (
            <button
              type="button"
              onClick={onDismiss}
              className="p-1 text-champagne/40 hover:text-champagne rounded transition-colors"
              aria-label="Dismiss task notification"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          )}
        </div>
      </div>

      {isRunning && <ProgressBar task={task} />}
    </div>
  );
}
