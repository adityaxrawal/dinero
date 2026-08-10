import { create } from 'zustand';
import { isTauriRuntime } from '@/lib/tauriRuntime';
import { mergeTask, type TaskUpdate } from './mergeTask';
import {
  API,
  type BackgroundTaskProgressPayload,
  type ScanProgressPayload,
  type SystemWarningPayload,
} from '@/lib/ipc';

export type NotificationCategory =
  | 'ingestion'
  | 'statements'
  | 'normalization'
  | 'database'
  | 'system';
export type NotificationSeverity = 'info' | 'success' | 'warning' | 'error';
export type TaskStatus = 'running' | 'completed' | 'failed' | 'cancelled' | 'cancelling';

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

  addOrUpdateTask: (incoming) => {
    const now = Date.now();
    set((state) => {
      const existing = state.tasks[incoming.id];
      // Do not overwrite a locally requested 'cancelling' state with a late-arriving progress update
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

  cancelTask: async (id) => {
    const task = get().tasks[id];
    if (!task) return;

    // Immediately mark cancelling locally
    get().addOrUpdateTask({
      id,
      title: task.title,
      category: task.category,
      status: 'cancelling',
      description: 'Cancelling operation…',
    });

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
      id: `notif_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`,
      timestamp: Date.now(),
      read: false,
      dismissed: false,
    };

    set((state) => ({
      // Keep up to 30 recent notifications
      notifications: [newItem, ...state.notifications].slice(0, 30),
    }));
  },

  dismissNotification: (id) =>
    set((state) => ({
      notifications: state.notifications.filter((n) => n.id !== id),
    })),

  clearAllNotifications: () => set({ notifications: [] }),

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

  fetchActiveTasks: async () => {
    if (!isTauriRuntime()) return;
    try {
      const active = await API.backgroundTasks.getActive();
      for (const task of active) {
        // Skip historical scan if handled by domain events to avoid duplication
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

  fetchActiveWarnings: async () => {
    if (!isTauriRuntime()) return;
    try {
      const active = await API.systemWarnings.getActive();
      for (const warning of active) {
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

// Setup automatic Tauri IPC event listeners
let listenersInitialized = false;

/**
 * Handlers read the store on each call rather than closing over a snapshot
 * taken at init: they outlive any single state value, and a captured
 * `getState()` result would go stale on the first update.
 */
const notifications = () => useNotificationStore.getState();

const SCAN_TASK_TITLE = 'Gmail Scan Pipeline';

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

// 1. Gmail / Historical Scan Events
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

// 2. Statement Batch & Processing Progress
function onStatementBatchProgress(p: StatementBatchPayload) {
  const isDone = p.parsed >= p.total;
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

// 3. AI Merchant & Category Normalization Cleanup Pass
function onMerchantCleanupProgress(p: MerchantCleanupPayload) {
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

// 4. Generic Background Tasks (Updater, Re-index, DB Vacuum)
const BACKGROUND_STATUS: Record<string, TaskStatus> = {
  running: 'running',
  completed: 'completed',
  failed: 'failed',
};

function onBackgroundTaskProgress(p: BackgroundTaskProgressPayload) {
  // Owned by the scan events above, which carry richer payloads.
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

// 5. System Warnings & Database Notifications
function onSystemWarning(w: SystemWarningPayload) {
  // Permission prompts are surfaced in-place by whatever asked for them.
  if (w.warning_type === 'keychain_denied' || w.warning_type === 'notification_denied') return;

  notifications().addNotification({
    category: 'system',
    severity: w.severity === 'critical' ? 'error' : w.severity === 'degraded' ? 'warning' : 'info',
    title: 'System Alert',
    message: w.message,
  });
}

function onDbBackupCompleted() {
  notifications().addNotification({
    category: 'database',
    severity: 'info',
    title: 'Database Backup Completed',
    message: 'An automatic snapshot of your financial ledger was created successfully.',
  });
}

function initNotificationStoreListeners() {
  if (listenersInitialized || !isTauriRuntime()) return;
  listenersInitialized = true;

  const store = useNotificationStore.getState();

  // Fetch initial state
  store.fetchActiveTasks();
  store.fetchActiveWarnings();

  (async () => {
    try {
      const { listen } = await import('@tauri-apps/api/event');

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

// Auto-init on module import in Tauri runtime
if (typeof window !== 'undefined' && isTauriRuntime()) {
  initNotificationStoreListeners();
}
