import { describe, it, expect, beforeEach, vi } from 'vitest';
import { useNotificationStore, type UnifiedTask } from './useNotificationStore';
import { API } from '@/lib/ipc';
import { isTauriRuntime } from '@/lib/tauriRuntime';

vi.mock('@/lib/tauriRuntime', () => ({ isTauriRuntime: vi.fn(() => false) }));
vi.mock('@/lib/ipc', () => ({
  API: {
    ingestion: { cancelScan: vi.fn() },
    backgroundTasks: { getActive: vi.fn() },
    systemWarnings: { getActive: vi.fn() },
  },
}));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;
const reset = () => useNotificationStore.setState({ tasks: {}, notifications: [], isExpanded: false });
const task = (id = 't1'): Pick<UnifiedTask, 'id' | 'title' | 'category'> => ({
  id,
  title: 'Gmail Scan',
  category: 'ingestion',
});

beforeEach(() => {
  reset();
  vi.clearAllMocks();
  asMock(isTauriRuntime).mockReturnValue(false);
  vi.spyOn(console, 'error').mockImplementation(() => {});
});

describe('addOrUpdateTask', () => {
  it('creates a task with defaults for everything unspecified', () => {
    useNotificationStore.getState().addOrUpdateTask(task());
    const t = useNotificationStore.getState().tasks.t1;
    expect(t).toMatchObject({
      status: 'running',
      current: 0,
      total: 0,
      progressPct: 0,
      description: '',
      cancelable: false,
      domainKey: 't1',
    });
  });

  it('derives progressPct from current/total', () => {
    useNotificationStore.getState().addOrUpdateTask({ ...task(), current: 25, total: 200 });
    expect(useNotificationStore.getState().tasks.t1.progressPct).toBe(13);
  });

  it('caps a derived progressPct at 100 when current overshoots total', () => {
    useNotificationStore.getState().addOrUpdateTask({ ...task(), current: 150, total: 100 });
    expect(useNotificationStore.getState().tasks.t1.progressPct).toBe(100);
  });

  it('prefers an explicit progressPct over the derived one', () => {
    useNotificationStore.getState().addOrUpdateTask({ ...task(), current: 1, total: 100, progressPct: 42 });
    expect(useNotificationStore.getState().tasks.t1.progressPct).toBe(42);
  });

  it('carries forward prior values on a partial update', () => {
    const s = useNotificationStore.getState();
    s.addOrUpdateTask({ ...task(), description: 'scanning', total: 100, cancelable: true });
    s.addOrUpdateTask({ ...task(), current: 50 });
    expect(useNotificationStore.getState().tasks.t1).toMatchObject({
      description: 'scanning',
      total: 100,
      cancelable: true,
      current: 50,
    });
  });

  it('preserves the original startedAt across updates', () => {
    const s = useNotificationStore.getState();
    s.addOrUpdateTask(task());
    const startedAt = useNotificationStore.getState().tasks.t1.startedAt;
    s.addOrUpdateTask({ ...task(), current: 5 });
    expect(useNotificationStore.getState().tasks.t1.startedAt).toBe(startedAt);
  });

  it('stamps finishedAt when a task reaches a terminal status', () => {
    const s = useNotificationStore.getState();
    s.addOrUpdateTask(task());
    expect(useNotificationStore.getState().tasks.t1.finishedAt).toBeFalsy();
    s.addOrUpdateTask({ ...task(), status: 'completed' });
    expect(useNotificationStore.getState().tasks.t1.finishedAt).toBeTruthy();
  });

  it('does not let a late progress event resurrect a cancelling task', () => {
    const s = useNotificationStore.getState();
    s.addOrUpdateTask({ ...task(), status: 'cancelling' });
    s.addOrUpdateTask({ ...task(), status: 'running', current: 99 });
    const t = useNotificationStore.getState().tasks.t1;
    expect(t.status).toBe('cancelling');
    expect(t.current).toBe(0);
  });

  it('still allows a cancelling task to reach cancelled', () => {
    const s = useNotificationStore.getState();
    s.addOrUpdateTask({ ...task(), status: 'cancelling' });
    s.addOrUpdateTask({ ...task(), status: 'cancelled' });
    expect(useNotificationStore.getState().tasks.t1.status).toBe('cancelled');
  });

  it('merges meta rather than replacing it', () => {
    const s = useNotificationStore.getState();
    s.addOrUpdateTask({ ...task(), meta: { account_id: 'acc1' } });
    s.addOrUpdateTask({ ...task(), meta: { run_id: 'r1' } });
    expect(useNotificationStore.getState().tasks.t1.meta).toEqual({ account_id: 'acc1', run_id: 'r1' });
  });

  it('distinguishes an explicit null etaSeconds from an omitted one', () => {
    const s = useNotificationStore.getState();
    s.addOrUpdateTask({ ...task(), etaSeconds: 120 });
    s.addOrUpdateTask({ ...task(), current: 1 });
    expect(useNotificationStore.getState().tasks.t1.etaSeconds).toBe(120);
    s.addOrUpdateTask({ ...task(), etaSeconds: null });
    expect(useNotificationStore.getState().tasks.t1.etaSeconds).toBeNull();
  });
});

describe('removeTask / clearCompletedTasks', () => {
  it('removes a single task', () => {
    const s = useNotificationStore.getState();
    s.addOrUpdateTask(task('a'));
    s.addOrUpdateTask(task('b'));
    s.removeTask('a');
    expect(Object.keys(useNotificationStore.getState().tasks)).toEqual(['b']);
  });

  it('keeps only running and cancelling tasks', () => {
    const s = useNotificationStore.getState();
    s.addOrUpdateTask({ ...task('run'), status: 'running' });
    s.addOrUpdateTask({ ...task('cancelling'), status: 'cancelling' });
    s.addOrUpdateTask({ ...task('done'), status: 'completed' });
    s.addOrUpdateTask({ ...task('failed'), status: 'failed' });
    s.clearCompletedTasks();
    expect(Object.keys(useNotificationStore.getState().tasks).sort()).toEqual(['cancelling', 'run']);
  });
});

describe('cancelTask', () => {
  it('ignores an unknown task id', async () => {
    await useNotificationStore.getState().cancelTask('nope');
    expect(API.ingestion.cancelScan).not.toHaveBeenCalled();
  });

  it('marks the task cancelling immediately', async () => {
    const s = useNotificationStore.getState();
    s.addOrUpdateTask({ ...task(), meta: { account_id: 'acc1' } });
    await s.cancelTask('t1');
    expect(useNotificationStore.getState().tasks.t1.status).toBe('cancelling');
  });

  it('asks the backend to cancel the scan behind the task', async () => {
    const s = useNotificationStore.getState();
    s.addOrUpdateTask({ ...task(), meta: { account_id: 'acc1' } });
    await s.cancelTask('t1');
    expect(API.ingestion.cancelScan).toHaveBeenCalledWith('acc1');
  });

  it('skips the backend call for a task with no account_id', async () => {
    const s = useNotificationStore.getState();
    s.addOrUpdateTask(task());
    await s.cancelTask('t1');
    expect(API.ingestion.cancelScan).not.toHaveBeenCalled();
    expect(useNotificationStore.getState().tasks.t1.status).toBe('cancelling');
  });

  it('stays cancelling even if the backend cancel fails', async () => {
    asMock(API.ingestion.cancelScan).mockRejectedValue(new Error('ipc down'));
    const s = useNotificationStore.getState();
    s.addOrUpdateTask({ ...task(), meta: { account_id: 'acc1' } });
    await expect(s.cancelTask('t1')).resolves.toBeUndefined();
    expect(useNotificationStore.getState().tasks.t1.status).toBe('cancelling');
  });
});

describe('notifications', () => {
  const item = { category: 'system' as const, severity: 'info' as const, title: 'T', message: 'M' };

  it('prepends new notifications', () => {
    const s = useNotificationStore.getState();
    s.addNotification({ ...item, title: 'first' });
    s.addNotification({ ...item, title: 'second' });
    expect(useNotificationStore.getState().notifications[0].title).toBe('second');
  });

  it('marks new notifications unread and undismissed', () => {
    useNotificationStore.getState().addNotification(item);
    expect(useNotificationStore.getState().notifications[0]).toMatchObject({ read: false, dismissed: false });
  });

  it('assigns unique ids', () => {
    const s = useNotificationStore.getState();
    for (let i = 0; i < 10; i++) s.addNotification(item);
    const ids = useNotificationStore.getState().notifications.map((n) => n.id);
    expect(new Set(ids).size).toBe(10);
  });

  it('keeps at most 30, dropping the oldest', () => {
    const s = useNotificationStore.getState();
    for (let i = 0; i < 35; i++) s.addNotification({ ...item, title: `n${i}` });
    const list = useNotificationStore.getState().notifications;
    expect(list).toHaveLength(30);
    expect(list[0].title).toBe('n34');
    expect(list[29].title).toBe('n5');
  });

  it('dismisses by id and clears all', () => {
    const s = useNotificationStore.getState();
    s.addNotification(item);
    const id = useNotificationStore.getState().notifications[0].id;
    s.dismissNotification(id);
    expect(useNotificationStore.getState().notifications).toHaveLength(0);
    s.addNotification(item);
    s.clearAllNotifications();
    expect(useNotificationStore.getState().notifications).toHaveLength(0);
  });
});

describe('expansion', () => {
  it('sets and toggles', () => {
    const s = useNotificationStore.getState();
    s.setExpanded(true);
    expect(useNotificationStore.getState().isExpanded).toBe(true);
    s.toggleExpanded();
    expect(useNotificationStore.getState().isExpanded).toBe(false);
  });
});

describe('fetchActiveTasks', () => {
  const bgTask = (over = {}) => ({
    task_id: 'bg1',
    task_type: 'db_vacuum',
    label: 'Vacuum',
    current: 3,
    total: 10,
    eta_seconds: 30,
    status: 'running',
    progress_pct: 30,
    status_message: 'working',
    ...over,
  });

  it('does nothing outside the Tauri runtime', async () => {
    await useNotificationStore.getState().fetchActiveTasks();
    expect(API.backgroundTasks.getActive).not.toHaveBeenCalled();
  });

  it('rehydrates in-progress tasks on mount', async () => {
    asMock(isTauriRuntime).mockReturnValue(true);
    asMock(API.backgroundTasks.getActive).mockResolvedValue([bgTask()]);
    await useNotificationStore.getState().fetchActiveTasks();
    expect(useNotificationStore.getState().tasks.bg1).toMatchObject({
      title: 'Vacuum',
      category: 'system',
      current: 3,
      progressPct: 30,
    });
  });

  it('routes statement tasks to the statements category', async () => {
    asMock(isTauriRuntime).mockReturnValue(true);
    asMock(API.backgroundTasks.getActive).mockResolvedValue([bgTask({ task_type: 'statement_import' })]);
    await useNotificationStore.getState().fetchActiveTasks();
    expect(useNotificationStore.getState().tasks.bg1.category).toBe('statements');
  });

  it('skips historical_scan, which the scan events already own', async () => {
    asMock(isTauriRuntime).mockReturnValue(true);
    asMock(API.backgroundTasks.getActive).mockResolvedValue([bgTask({ task_type: 'historical_scan' })]);
    await useNotificationStore.getState().fetchActiveTasks();
    expect(useNotificationStore.getState().tasks).toEqual({});
  });

  it('swallows a backend failure rather than breaking mount', async () => {
    asMock(isTauriRuntime).mockReturnValue(true);
    asMock(API.backgroundTasks.getActive).mockRejectedValue(new Error('ipc down'));
    await expect(useNotificationStore.getState().fetchActiveTasks()).resolves.toBeUndefined();
  });
});

describe('fetchActiveWarnings', () => {
  const warning = (over = {}) => ({
    warning_type: 'gmail_rate_limited',
    message: 'Gmail is rate limiting',
    severity: 'degraded',
    action_hint: null,
    ...over,
  });

  it('does nothing outside the Tauri runtime', async () => {
    await useNotificationStore.getState().fetchActiveWarnings();
    expect(API.systemWarnings.getActive).not.toHaveBeenCalled();
  });

  it.each([
    ['critical', 'error'],
    ['degraded', 'warning'],
    ['info', 'info'],
  ])('maps %s severity to %s', async (severity, expected) => {
    asMock(isTauriRuntime).mockReturnValue(true);
    asMock(API.systemWarnings.getActive).mockResolvedValue([warning({ severity })]);
    await useNotificationStore.getState().fetchActiveWarnings();
    expect(useNotificationStore.getState().notifications[0].severity).toBe(expected);
  });

  it.each(['keychain_denied', 'notification_denied'])(
    'suppresses %s, which has its own dedicated UI',
    async (warning_type) => {
      asMock(isTauriRuntime).mockReturnValue(true);
      asMock(API.systemWarnings.getActive).mockResolvedValue([warning({ warning_type })]);
      await useNotificationStore.getState().fetchActiveWarnings();
      expect(useNotificationStore.getState().notifications).toHaveLength(0);
    }
  );

  it('swallows a backend failure', async () => {
    asMock(isTauriRuntime).mockReturnValue(true);
    asMock(API.systemWarnings.getActive).mockRejectedValue(new Error('ipc down'));
    await expect(useNotificationStore.getState().fetchActiveWarnings()).resolves.toBeUndefined();
  });
});
