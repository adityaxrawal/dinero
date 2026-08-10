import { describe, it, expect, beforeAll, beforeEach, vi } from 'vitest';
import type { useSyncStore as StoreType } from '@/stores/useSyncStore';

// The event subscription and post-reload re-hydration both live in a
// module-level IIFE that only runs inside the Tauri shell, so this file fakes
// __TAURI_INTERNALS__ and imports the module fresh to reach them.
const handlers = new Map<string, (event: { payload: unknown }) => void>();
const listConnectedAccounts = vi.fn();
const getScanStatus = vi.fn();

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((name: string, cb: (event: { payload: unknown }) => void) => {
    handlers.set(name, cb);
    return Promise.resolve(() => {});
  }),
}));

vi.mock('@/lib/ipc', () => ({
  API: {
    auth: { listConnectedAccounts: () => listConnectedAccounts() },
    ingestion: { getScanStatus: (id: string) => getScanStatus(id) },
    systemWarnings: { dismiss: vi.fn().mockResolvedValue(undefined) },
  },
}));

const inProgress = (over = {}) => ({
  status: 'in_progress',
  processed: 40,
  total: 100,
  transactions_found: 5,
  statements_found: 2,
  mandate_events_found: 0,
  errors: 0,
  pending_enrichment: 1,
  ...over,
});

let store: typeof StoreType;
const emit = (event: string, payload: unknown) => handlers.get(event)!({ payload });

beforeAll(async () => {
  (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {};
  // One account hydrates, one throws — the throwing one must not block it.
  listConnectedAccounts.mockResolvedValue([{ account_id: 'acc1' }, { account_id: 'acc2' }]);
  getScanStatus.mockImplementation((id: string) => {
    if (id === 'acc2') return Promise.reject(new Error('unreadable'));
    return Promise.resolve(inProgress());
  });
  vi.spyOn(console, 'error').mockImplementation(() => {});

  vi.resetModules();
  ({ useSyncStore: store } = await import('@/stores/useSyncStore'));
  await vi.waitFor(() => expect(handlers.has('system_warning')).toBe(true));
});

describe('re-hydration after a webview reload', () => {
  it('restores an in-progress scan without waiting for the next event', async () => {
    await vi.waitFor(() => expect(store.getState().scanStatus).toBe('running'));
    expect(store.getState().scanProgress).toMatchObject({
      account_id: 'acc1',
      processed: 40,
      total: 100,
    });
  });

  it('queried every connected account, tolerating one that failed', () => {
    expect(getScanStatus).toHaveBeenCalledWith('acc1');
    expect(getScanStatus).toHaveBeenCalledWith('acc2');
  });
});

describe('subscribed sync events', () => {
  beforeEach(() =>
    store.setState({ scanStatus: 'idle', scanProgress: null, scanError: null, warnings: [] })
  );

  it('mirrors scan_progress into the store', () => {
    emit('scan_progress', { account_id: 'acc1', processed: 7, total: 20 });
    expect(store.getState().scanStatus).toBe('running');
    expect(store.getState().scanProgress).toMatchObject({ processed: 7 });
  });

  it('marks the scan done on scan_completed', () => {
    emit('scan_completed', {});
    expect(store.getState().scanStatus).toBe('done');
  });

  it('records the reported failure message', () => {
    emit('scan_failed', { error_message: 'Gmail rate limited' });
    expect(store.getState()).toMatchObject({ scanStatus: 'error', scanError: 'Gmail rate limited' });
  });

  it('falls back to a generic message when none is reported', () => {
    emit('scan_failed', {});
    expect(store.getState().scanError).toBe('Scan failed');
  });

  it('appends system warnings', () => {
    emit('system_warning', { warning_type: 'low_memory', message: 'Low RAM' });
    expect(store.getState().warnings).toHaveLength(1);
    expect(store.getState().warnings[0].warning_type).toBe('low_memory');
  });
});
