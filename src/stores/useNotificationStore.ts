import { create } from 'zustand';
import { isTauriRuntime } from '@/lib/tauriRuntime';
import { mergeTask, type TaskUpdate } from './mergeTask';
import {
  API,
  type BackgroundTaskProgressPayload,
  type ScanProgressPayload,
  type SystemWarningPayload,
} from '@/lib/ipc';

/**
 * Backs the sidebar notification centre: live tasks plus a historical feed.
 *
 * Two distinct collections with different lifetimes are held side by side, and
 * the distinction is the core idea of this module:
 *
 *   - `tasks` is a keyed map of work currently happening or just finished. Each
 *     entry is updated in place as progress events arrive, so a scan occupies
 *     one row that advances rather than accumulating hundreds of rows.
 *   - `notifications` is an append-only feed of discrete events worth reporting
 *     after the fact, capped so it cannot grow without bound.
 *
 * Every background activity in the app funnels through here -- Gmail scans,
 * statement batches, merchant cleanup, system warnings -- normalised into the
 * single UnifiedTask shape so one component can render them all.
 */
export type NotificationCategory =
  | 'ingestion'
  | 'statements'
  | 'normalization'
  | 'database'
  | 'system';
export type NotificationSeverity = 'info' | 'success' | 'warning' | 'error';
export type TaskStatus = 'running' | 'completed' | 'failed' | 'cancelled' | 'cancelling';

/**
 * One unit of background work, whatever produced it.
 *
 * `domainKey` groups related tasks for display, while `id` remains the unique
 * update key -- the two differ so several tasks can collapse under one heading
 * without colliding. `meta` carries source-specific fields the generic shape has
 * no room for, such as the account id needed to cancel a scan.
 */
export interface UnifiedTask {
  id: string;
  domainKey: string;
  category: NotificationCategory;
  title: string;
  description: string;
  status: TaskStatus;
  current: number;
  total: number;
  progressPct: number;
  etaSeconds?: number | null | undefined;
  startedAt: number;
  updatedAt: number;
  finishedAt?: number | null | undefined;
  errorMessage?: string | null | undefined;
  cancelable?: boolean;
  meta?: Record<string, unknown>;
}

/**
 * One entry in the historical feed.
 *
 * `taskId` optionally links an entry back to the task that produced it, which is
 * what lets a finished task's outcome be traced from the feed.
 */
export interface NotificationFeedItem {
  id: string;
  category: NotificationCategory;
  severity: NotificationSeverity;
  title: string;
  message: string;
  timestamp: number;
  read: boolean;
  dismissed: boolean;
  actionUrl?: string;
  actionLabel?: string;
  taskId?: string;
}

interface NotificationStoreState {
  tasks: Record<string, UnifiedTask>;
  notifications: NotificationFeedItem[];
  isExpanded: boolean;

  setExpanded: (expanded: boolean) => void;
  toggleExpanded: () => void;

  addOrUpdateTask: (task: TaskUpdate) => void;
  removeTask: (id: string) => void;
  cancelTask: (id: string) => Promise<void>;

  addNotification: (
    item: Omit<NotificationFeedItem, 'id' | 'timestamp' | 'read' | 'dismissed'>
  ) => void;
  dismissNotification: (id: string) => void;
  clearAllNotifications: () => void;
  clearCompletedTasks: () => void;

  fetchActiveTasks: () => Promise<void>;
  fetchActiveWarnings: () => Promise<void>;
}

export const useNotificationStore = create<NotificationStoreState>((set, get) => ({
  tasks: {},
  notifications: [],
  isExpanded: false,

  setExpanded: (expanded: boolean) => set({ isExpanded: expanded }),
  toggleExpanded: () => set((state) => ({ isExpanded: !state.isExpanded })),

  /**
   * Insert or advance a task, delegating the field-level merge to mergeTask.
   *
   * The guard below resolves a genuine race: after the user clicks cancel the
   * task is marked 'cancelling', but progress events already in flight would
   * otherwise flip it back to 'running' and make the button appear to have done
   * nothing. Those late events are dropped until the backend confirms.
   */
  addOrUpdateTask: (incoming) => {
    const now = Date.now();
    set((state) => {
      const existing = state.tasks[incoming.id];
      if (existing && existing.status === 'cancelling' && incoming.status === 'running') {
        return state;
      }

      return {
        tasks: {
          ...state.tasks,
          [incoming.id]: mergeTask(existing, incoming, now),
        },
      };
    });
  },

  removeTask: (id) =>
    set((state) => {
      const nextTasks = { ...state.tasks };
      delete nextTasks[id];
      return { tasks: nextTasks };
    }),

  /**
   * Request cancellation of a task.
   *
   * The UI is updated to 'cancelling' first so the click registers immediately,
   * then the backend is asked to stop. The final status arrives later as an
   * event; this never declares the task cancelled on its own.
   */
  cancelTask: async (id) => {
    const task = get().tasks[id];
    if (!task) return;

    get().addOrUpdateTask({
      id,
      title: task.title,
      category: task.category,
      status: 'cancelling',
      description: 'Cancelling operation…',
    });

    // Only scans are cancellable today, and they are identified by the
    // account_id their meta carries. Other task types simply stay in the
    // 'cancelling' state until they finish naturally.
    if (task.meta?.account_id && typeof task.meta.account_id === 'string') {
      try {
        await API.ingestion.cancelScan(task.meta.account_id);
      } catch (err) {
        console.error('Failed to cancel scan:', err);
      }
    }
  },

  addNotification: (item) => {
    const newItem: NotificationFeedItem = {
      ...item,
      // Timestamp plus a short random suffix: several notifications can be
      // raised within the same millisecond, so the clock alone is not unique.
      id: `notif_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`,
      timestamp: Date.now(),
      read: false,
      dismissed: false,
    };

    // Newest first, hard-capped at 30. The cap is what keeps this an unbounded
    // event source without an unbounded array behind it.
    set((state) => ({
      notifications: [newItem, ...state.notifications].slice(0, 30),
    }));
  },

  dismissNotification: (id) =>
    set((state) => ({
      notifications: state.notifications.filter((n) => n.id !== id),
    })),

  clearAllNotifications: () => set({ notifications: [] }),

  /**
   * Drop finished tasks, keeping anything still in flight.
   *
   * Rebuilt as a fresh object rather than filtered in place, so subscribers see
   * a new reference and re-render. Note 'cancelling' counts as active -- the
   * task has not actually stopped yet.
   */
  clearCompletedTasks: () =>
    set((state) => {
      const activeOnly: Record<string, UnifiedTask> = {};
      for (const [id, task] of Object.entries(state.tasks)) {
        if (task.status === 'running' || task.status === 'cancelling') {
          activeOnly[id] = task;
        }
      }
      return { tasks: activeOnly };
    }),

  /**
   * Adopt background tasks already running in the backend at startup.
   *
   * Backend work survives a frontend reload, so without this the notification
   * centre would show nothing while a task continued invisibly.
   */
  fetchActiveTasks: async () => {
    if (!isTauriRuntime()) return;
    try {
      const active = await API.backgroundTasks.getActive();
      for (const task of active) {
        // Scans are excluded here because the scan_progress event path already
        // owns them; adopting them here too would create a duplicate row.
        if (task.task_type === 'historical_scan') continue;

        get().addOrUpdateTask({
          id: task.task_id,
          domainKey: `bg_task:${task.task_id}`,
          category: task.task_type.includes('statement') ? 'statements' : 'system',
          title: task.label,
          description: task.status_message,
          status: task.status as TaskStatus,
          current: task.current,
          total: task.total,
          progressPct: task.progress_pct,
          etaSeconds: task.eta_seconds,
        });
      }
    } catch (err) {
      console.error('Failed to fetch active background tasks:', err);
    }
  },

  /**
   * Pull outstanding system warnings into the feed at startup.
   *
   * Backend severity levels are mapped onto the feed's own severity vocabulary,
   * since the two scales are not the same set of names.
   */
  fetchActiveWarnings: async () => {
    if (!isTauriRuntime()) return;
    try {
      const active = await API.systemWarnings.getActive();
      for (const warning of active) {
        // Permission-denial warnings are skipped: they already have dedicated,
        // more actionable UI (a full overlay explaining how to grant access),
        // so surfacing them again here would just duplicate that.
        if (
          warning.warning_type === 'keychain_denied' ||
          warning.warning_type === 'notification_denied'
        ) {
          continue;
        }
        get().addNotification({
          category: 'system',
          severity:
            warning.severity === 'critical'
              ? 'error'
              : warning.severity === 'degraded'
                ? 'warning'
                : 'info',
          title: 'System Alert',
          message: warning.message,
        });
      }
    } catch (err) {
      console.error('Failed to fetch system warnings:', err);
    }
  },
}));

// ---------------------------------------------------------------------------
// Event adapters
//
// Each handler below translates one backend event into the store's vocabulary.
// The shared convention is that a task id is derived deterministically from the
// thing being worked on (the account, the batch, the cleanup run) rather than
// generated -- that is what makes repeated progress events update one row
// instead of creating new ones. Terminal handlers additionally append a feed
// entry, so a finished task leaves a durable record after its live row is
// cleared.
// ---------------------------------------------------------------------------

// Guards against double-subscription if the init function is ever called twice.
let listenersInitialized = false;

// Read the store imperatively; these run outside React.
const notifications = () => useNotificationStore.getState();

// One constant so every scan event renders under an identical heading, which is
// what lets progress, completion and failure collapse into a single row.
const SCAN_TASK_TITLE = 'Gmail Scan Pipeline';

// Both message fields are optional and either may carry the text, so the
// handler falls back across them rather than assuming one shape.
interface ScanFailedPayload {
  error?: string;
  error_message?: string;
  account_id?: string;
}

interface StatementBatchPayload {
  parsed: number;
  total: number;
  eta_seconds: number;
}

interface MerchantCleanupPayload {
  run_id: string;
  processed: number;
  total: number;
  applied: number;
  skipped: number;
  current_merchant: string | null;
  status: 'running' | 'completed' | 'cancelled' | 'failed';
}

/**
 * Live scan progress. Marked cancelable and carries the account id in meta,
 * which is what the cancel action later reads to address the right scan.
 */
function onScanProgress(p: ScanProgressPayload) {
  notifications().addOrUpdateTask({
    id: `scan:${p.account_id}`,
    domainKey: `scan:${p.account_id}`,
    category: 'ingestion',
    title: SCAN_TASK_TITLE,
    description: `Found ${p.transactions_found} txns, ${p.statements_found} stmts`,
    status: 'running',
    current: p.processed,
    total: p.total,
    cancelable: true,
    meta: { account_id: p.account_id },
  });
}

/**
 * Successful completion: close out the task row, then record a feed entry.
 *
 * Progress is forced to 100% and current to total, because the final event's
 * counters can lag slightly behind and would otherwise leave the bar short of
 * full on a scan that actually finished.
 */
function onScanCompleted(p: ScanProgressPayload) {
  const taskId = `scan:${p.account_id}`;
  notifications().addOrUpdateTask({
    id: taskId,
    domainKey: taskId,
    category: 'ingestion',
    title: SCAN_TASK_TITLE,
    description: `Completed: ${p.transactions_found} txns, ${p.statements_found} stmts found`,
    status: 'completed',
    current: p.total,
    total: p.total,
    progressPct: 100,
  });

  notifications().addNotification({
    category: 'ingestion',
    severity: 'success',
    title: 'Gmail Scan Completed',
    message: `Processed ${p.total} emails. Found ${p.transactions_found} transactions and ${p.statements_found} statements.`,
    actionUrl: '/transactions',
    actionLabel: 'View Transactions',
    taskId,
  });
}

/**
 * Failure: mark the task failed and post an error to the feed.
 *
 * Both the message and the account id are defaulted, since a failure early
 * enough in the pipeline may not know which account it was working on -- and a
 * failure with no id must still produce a visible, addressable task row.
 */
function onScanFailed(p: ScanFailedPayload) {
  const errStr = p.error ?? p.error_message ?? 'Scan failed';
  const accountId = p.account_id ?? 'primary';
  const taskId = `scan:${accountId}`;

  notifications().addOrUpdateTask({
    id: taskId,
    domainKey: taskId,
    category: 'ingestion',
    title: SCAN_TASK_TITLE,
    description: errStr,
    status: 'failed',
    errorMessage: errStr,
  });

  notifications().addNotification({
    category: 'ingestion',
    severity: 'error',
    title: 'Gmail Scan Failed',
    message: errStr,
    taskId,
  });
}

/**
 * User-initiated cancellation. Logged at info rather than error severity, and
 * the feed message records how far the scan reached before stopping.
 */
function onScanCancelled(p: ScanProgressPayload) {
  const taskId = `scan:${p.account_id}`;

  notifications().addOrUpdateTask({
    id: taskId,
    domainKey: taskId,
    category: 'ingestion',
    title: SCAN_TASK_TITLE,
    description: 'Scan cancelled by user',
    status: 'cancelled',
  });

  notifications().addNotification({
    category: 'ingestion',
    severity: 'info',
    title: 'Gmail Scan Cancelled',
    message: `Scan stopped at ${p.processed}/${p.total} emails.`,
    taskId,
  });
}

/**
 * Statement batch import, carrying both progress and completion.
 *
 * Unlike scans, the backend signals no separate completion event here --
 * `parsed >= total` is the completion condition, which is why one handler covers
 * both the running and finished states.
 */
function onStatementBatchProgress(p: StatementBatchPayload) {
  const isDone = p.parsed >= p.total;
  // A fixed id, not a derived one: only one batch import runs at a time.
  const taskId = 'statement_batch_pipeline';

  notifications().addOrUpdateTask({
    id: taskId,
    domainKey: taskId,
    category: 'statements',
    title: 'Statement Import Pipeline',
    description: isDone
      ? `Finished processing ${p.total} statements`
      : `Importing PDF statements (${p.parsed}/${p.total})`,
    status: isDone ? 'completed' : 'running',
    current: p.parsed,
    total: p.total,
    etaSeconds: p.eta_seconds,
  });

  if (isDone) {
    notifications().addNotification({
      category: 'statements',
      severity: 'success',
      title: 'Statement Batch Complete',
      message: `Successfully processed ${p.total} PDF statement file${p.total === 1 ? '' : 's'}.`,
      actionUrl: '/statements',
      actionLabel: 'View Statements',
      taskId,
    });
  }
}

/**
 * AI merchant normalisation progress.
 *
 * The description escalates in specificity: the finished summary if done, else
 * the merchant currently being processed, else a bare counter. That ordering
 * means the row always says the most informative thing available at that moment.
 */
function onMerchantCleanupProgress(p: MerchantCleanupPayload) {
  // Keyed by run id, so a re-run creates its own row rather than overwriting
  // the record of the previous one.
  const taskId = `merchant_cleanup:${p.run_id}`;
  const isDone = p.status === 'completed';
  const isFailed = p.status === 'failed';
  const isCancelled = p.status === 'cancelled';

  notifications().addOrUpdateTask({
    id: taskId,
    domainKey: taskId,
    category: 'normalization',
    title: 'AI Merchant Normalization',
    description: isDone
      ? `Normalized ${p.applied} merchants`
      : p.current_merchant
        ? `Cleaning: ${p.current_merchant}`
        : `Processing transactions (${p.processed}/${p.total})`,
    status: isDone ? 'completed' : isFailed ? 'failed' : isCancelled ? 'cancelled' : 'running',
    current: p.processed,
    total: p.total,
  });

  if (isDone) {
    notifications().addNotification({
      category: 'normalization',
      severity: 'success',
      title: 'Normalization Complete',
      message: `AI pass finished: ${p.applied} transactions normalized & categorized.`,
      actionUrl: '/settings',
      actionLabel: 'View Settings',
      taskId,
    });
  }
}

// Backend task status onto this store's TaskStatus. Explicit rather than a
// cast, so an unrecognised backend status falls back to 'running' below instead
// of entering the store as an invalid value.
const BACKGROUND_STATUS: Record<string, TaskStatus> = {
  running: 'running',
  completed: 'completed',
  failed: 'failed',
};

/**
 * Generic background task progress, covering every task type that does not have
 * a dedicated handler above.
 */
function onBackgroundTaskProgress(p: BackgroundTaskProgressPayload) {
  // Scans arrive on their own event stream and are handled there; ignoring them
  // here is what prevents the same scan appearing as two separate rows.
  if (p.task_type === 'historical_scan') return;

  const taskId = p.task_id;

  notifications().addOrUpdateTask({
    id: taskId,
    domainKey: `bg_task:${taskId}`,
    category: p.task_type.includes('statement') ? 'statements' : 'system',
    title: p.label,
    description: p.status_message,
    status: BACKGROUND_STATUS[p.status] ?? 'running',
    current: p.current,
    total: p.total,
    progressPct: p.progress_pct,
    etaSeconds: p.eta_seconds,
  });

  if (p.status === 'failed') {
    notifications().addNotification({
      category: 'system',
      severity: 'error',
      title: `Task Failed: ${p.label}`,
      message: p.status_message,
      taskId,
    });
  }
}

/**
 * System warning pushed by the backend. Goes straight to the feed with no task
 * row, since a warning is a discrete event rather than ongoing work.
 */
function onSystemWarning(w: SystemWarningPayload) {
  // Same exclusion as the startup fetch: these two have dedicated overlays.
  if (w.warning_type === 'keychain_denied' || w.warning_type === 'notification_denied') return;

  notifications().addNotification({
    category: 'system',
    severity: w.severity === 'critical' ? 'error' : w.severity === 'degraded' ? 'warning' : 'info',
    title: 'System Alert',
    message: w.message,
  });
}

/** Automatic backup finished. Informational only -- no task row, no action. */
function onDbBackupCompleted() {
  notifications().addNotification({
    category: 'database',
    severity: 'info',
    title: 'Database Backup Completed',
    message: 'An automatic snapshot of your financial ledger was created successfully.',
  });
}

/**
 * Wire the store to the backend: adopt existing work, then subscribe to events.
 *
 * The flag makes this idempotent, so a repeated call cannot double-subscribe
 * and cause every task to be updated twice per event.
 */
function initNotificationStoreListeners() {
  if (listenersInitialized || !isTauriRuntime()) return;
  listenersInitialized = true;

  const store = useNotificationStore.getState();

  // Catch up on work already in progress before live events start arriving.
  store.fetchActiveTasks();
  store.fetchActiveWarnings();

  (async () => {
    try {
      const { listen } = await import('@tauri-apps/api/event');

      // These subscriptions are never released -- the store is a module-level
      // singleton that lives as long as the window does.
      await listen<ScanProgressPayload>('scan_progress', (e) => onScanProgress(e.payload));
      await listen<ScanProgressPayload>('scan_completed', (e) => onScanCompleted(e.payload));
      await listen<ScanFailedPayload>('scan_failed', (e) => onScanFailed(e.payload));
      await listen<ScanProgressPayload>('scan_cancelled', (e) => onScanCancelled(e.payload));
      await listen<StatementBatchPayload>('statement_batch_progress', (e) =>
        onStatementBatchProgress(e.payload)
      );
      await listen<MerchantCleanupPayload>('merchant_cleanup_progress', (e) =>
        onMerchantCleanupProgress(e.payload)
      );
      await listen<BackgroundTaskProgressPayload>('background_task_progress', (e) =>
        onBackgroundTaskProgress(e.payload)
      );
      await listen<SystemWarningPayload>('system_warning', (e) => onSystemWarning(e.payload));
      await listen('db_backup_completed', onDbBackupCompleted);
    } catch (err) {
      console.error('Failed to set up notification listeners:', err);
    }
  })();
}

// Self-initialise on import. The window check keeps this inert under any
// non-DOM environment, and the runtime check keeps it inert in a plain browser.
if (typeof window !== 'undefined' && isTauriRuntime()) {
  initNotificationStoreListeners();
}
