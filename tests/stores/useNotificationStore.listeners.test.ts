import { describe, it, expect, beforeAll, beforeEach, vi } from 'vitest';
import type { useNotificationStore as StoreType } from '@/stores/useNotificationStore';

// The listener wiring runs once, at module import, and only inside the Tauri
// shell. Faking __TAURI_INTERNALS__ before a fresh import is the only way to
// reach these handlers — they are not exported.
const handlers = new Map<string, (event: { payload: unknown }) => void>();

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((name: string, cb: (event: { payload: unknown }) => void) => {
    handlers.set(name, cb);
    return Promise.resolve(() => {});
  }),
}));

vi.mock('@/lib/ipc', () => ({
  API: {
    ingestion: { cancelScan: vi.fn() },
    backgroundTasks: { getActive: vi.fn().mockResolvedValue([]) },
    systemWarnings: { getActive: vi.fn().mockResolvedValue([]) },
  },
}));

let store: typeof StoreType;
const emit = (event: string, payload: unknown) => {
  const handler = handlers.get(event);
  if (!handler) throw new Error(`no listener registered for "${event}"`);
  handler({ payload });
};

beforeAll(async () => {
  (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {};
  vi.resetModules();
  ({ useNotificationStore: store } = await import('@/stores/useNotificationStore'));
  // The listener registration is an async IIFE; let it settle.
  await vi.waitFor(() => expect(handlers.has('db_backup_completed')).toBe(true));
});

beforeEach(() => store.setState({ tasks: {}, notifications: [], isExpanded: false }));

const scan = (over = {}) => ({
  account_id: 'acc1',
  processed: 10,
  total: 100,
  transactions_found: 3,
  statements_found: 1,
  ...over,
});

describe('scan events', () => {
  it('tracks progress as a cancelable ingestion task', () => {
    emit('scan_progress', scan());
    const task = store.getState().tasks['scan:acc1'];
    expect(task).toMatchObject({ category: 'ingestion', status: 'running', current: 10, cancelable: true });
    expect(task.meta).toEqual({ account_id: 'acc1' });
  });

  it('completes the task and posts a success notification', () => {
    emit('scan_progress', scan());
    emit('scan_completed', scan({ processed: 100 }));
    expect(store.getState().tasks['scan:acc1']).toMatchObject({ status: 'completed', progressPct: 100 });
    expect(store.getState().notifications[0]).toMatchObject({
      severity: 'success',
      title: 'Gmail Scan Completed',
      actionUrl: '/transactions',
    });
  });

  it('records a failure with the error message', () => {
    emit('scan_failed', { error: 'rate limited', account_id: 'acc1' });
    expect(store.getState().tasks['scan:acc1']).toMatchObject({ status: 'failed', errorMessage: 'rate limited' });
    expect(store.getState().notifications[0].severity).toBe('error');
  });

  it('accepts the alternate error_message field', () => {
    emit('scan_failed', { error_message: 'token expired', account_id: 'acc1' });
    expect(store.getState().tasks['scan:acc1'].errorMessage).toBe('token expired');
  });

  it('falls back to a generic message and the primary account', () => {
    emit('scan_failed', {});
    expect(store.getState().tasks['scan:primary'].errorMessage).toBe('Scan failed');
  });

  it('reports where a cancelled scan stopped', () => {
    emit('scan_cancelled', scan({ processed: 42 }));
    expect(store.getState().tasks['scan:acc1'].status).toBe('cancelled');
    expect(store.getState().notifications[0].message).toContain('42/100');
  });
});

describe('statement_batch_progress', () => {
  it('tracks an in-flight batch', () => {
    emit('statement_batch_progress', { parsed: 2, total: 5, eta_seconds: 30 });
    expect(store.getState().tasks.statement_batch_pipeline).toMatchObject({
      category: 'statements',
      status: 'running',
      current: 2,
      etaSeconds: 30,
    });
    expect(store.getState().notifications).toHaveLength(0);
  });

  it('completes and notifies once every file is parsed', () => {
    emit('statement_batch_progress', { parsed: 5, total: 5, eta_seconds: 0 });
    expect(store.getState().tasks.statement_batch_pipeline.status).toBe('completed');
    expect(store.getState().notifications[0]).toMatchObject({
      title: 'Statement Batch Complete',
      actionUrl: '/statements',
    });
  });

  it('uses the singular noun for a one-file batch', () => {
    emit('statement_batch_progress', { parsed: 1, total: 1, eta_seconds: 0 });
    expect(store.getState().notifications[0].message).toContain('1 PDF statement file.');
  });
});

describe('merchant_cleanup_progress', () => {
  const cleanup = (over = {}) => ({
    run_id: 'r1',
    processed: 5,
    total: 10,
    applied: 3,
    skipped: 2,
    current_merchant: null,
    status: 'running',
    ...over,
  });

  it('names the merchant currently being cleaned', () => {
    emit('merchant_cleanup_progress', cleanup({ current_merchant: 'AMZN MKTP' }));
    expect(store.getState().tasks['merchant_cleanup:r1'].description).toBe('Cleaning: AMZN MKTP');
  });

  it('falls back to a counter when no merchant is named', () => {
    emit('merchant_cleanup_progress', cleanup());
    expect(store.getState().tasks['merchant_cleanup:r1'].description).toBe('Processing transactions (5/10)');
  });

  it('summarises and notifies on completion', () => {
    emit('merchant_cleanup_progress', cleanup({ status: 'completed', applied: 7 }));
    expect(store.getState().tasks['merchant_cleanup:r1']).toMatchObject({
      status: 'completed',
      description: 'Normalized 7 merchants',
    });
    expect(store.getState().notifications[0].title).toBe('Normalization Complete');
  });

  it.each(['failed', 'cancelled'])('mirrors a %s run without notifying', (status) => {
    emit('merchant_cleanup_progress', cleanup({ status }));
    expect(store.getState().tasks['merchant_cleanup:r1'].status).toBe(status);
    expect(store.getState().notifications).toHaveLength(0);
  });
});

describe('background_task_progress', () => {
  const bg = (over = {}) => ({
    task_id: 'bg1',
    task_type: 'db_vacuum',
    label: 'Vacuum',
    current: 1,
    total: 4,
    eta_seconds: 10,
    status: 'running',
    progress_pct: 25,
    status_message: 'working',
    ...over,
  });

  it('tracks a generic system task', () => {
    emit('background_task_progress', bg());
    expect(store.getState().tasks.bg1).toMatchObject({ category: 'system', status: 'running', progressPct: 25 });
  });

  it('routes statement task types to the statements category', () => {
    emit('background_task_progress', bg({ task_type: 'statement_reparse' }));
    expect(store.getState().tasks.bg1.category).toBe('statements');
  });

  it('ignores historical_scan, owned by the scan events', () => {
    emit('background_task_progress', bg({ task_type: 'historical_scan' }));
    expect(store.getState().tasks).toEqual({});
  });

  it('raises an error notification on failure', () => {
    emit('background_task_progress', bg({ status: 'failed', status_message: 'disk full' }));
    expect(store.getState().notifications[0]).toMatchObject({
      severity: 'error',
      title: 'Task Failed: Vacuum',
      message: 'disk full',
    });
  });

  it('treats an unrecognised status as still running', () => {
    emit('background_task_progress', bg({ status: 'queued' }));
    expect(store.getState().tasks.bg1.status).toBe('running');
  });
});

describe('system_warning and db_backup_completed', () => {
  it.each([
    ['critical', 'error'],
    ['degraded', 'warning'],
    ['info', 'info'],
  ])('maps %s severity to %s', (severity, expected) => {
    emit('system_warning', { warning_type: 'low_memory', message: 'Low RAM', severity, action_hint: null });
    expect(store.getState().notifications[0].severity).toBe(expected);
  });

  it.each(['keychain_denied', 'notification_denied'])('suppresses %s', (warning_type) => {
    emit('system_warning', { warning_type, message: 'denied', severity: 'critical', action_hint: null });
    expect(store.getState().notifications).toHaveLength(0);
  });

  it('announces a completed backup', () => {
    emit('db_backup_completed', {});
    expect(store.getState().notifications[0]).toMatchObject({
      category: 'database',
      title: 'Database Backup Completed',
    });
  });
});
