// Progress events arrive partial and out of order; these pin that a later
// event never blanks out detail an earlier one supplied.
import { describe, it, expect } from 'vitest';
import { mergeTask } from './mergeTask';
import type { UnifiedTask } from './useNotificationStore';

const NOW = 1_700_000_000_000;

const incoming = (over = {}) =>
  ({ id: 't1', title: 'Gmail Scan', category: 'ingestion' as const, ...over });

const existing = (over: Partial<UnifiedTask> = {}): UnifiedTask => ({
  id: 't1',
  domainKey: 'scan:acc1',
  category: 'ingestion',
  title: 'Gmail Scan',
  description: 'Found 7 txns',
  status: 'running',
  current: 10,
  total: 100,
  progressPct: 10,
  etaSeconds: 42,
  startedAt: NOW - 60_000,
  updatedAt: NOW - 1_000,
  finishedAt: null,
  errorMessage: null,
  cancelable: true,
  meta: { account_id: 'acc1' },
  ...over,
});

describe('mergeTask on a first sighting', () => {
  it('fills sensible defaults for everything the event omits', () => {
    const task = mergeTask(undefined, incoming(), NOW);
    expect(task).toMatchObject({
      domainKey: 't1',
      description: '',
      status: 'running',
      current: 0,
      total: 0,
      progressPct: 0,
      etaSeconds: null,
      errorMessage: null,
      cancelable: false,
      startedAt: NOW,
      meta: {},
    });
  });

  it('derives a percentage from current/total', () => {
    expect(mergeTask(undefined, incoming({ current: 25, total: 200 }), NOW).progressPct).toBe(13);
  });

  it('never reports over 100% if the backend overshoots', () => {
    expect(mergeTask(undefined, incoming({ current: 250, total: 100 }), NOW).progressPct).toBe(100);
  });

  it('prefers an explicit percentage over the derived one', () => {
    expect(
      mergeTask(undefined, incoming({ current: 1, total: 100, progressPct: 90 }), NOW).progressPct
    ).toBe(90);
  });
});

describe('mergeTask onto a known task', () => {
  it('keeps the original start time and stamps the update time', () => {
    const task = mergeTask(existing(), incoming({ current: 20 }), NOW);
    expect(task.startedAt).toBe(NOW - 60_000);
    expect(task.updatedAt).toBe(NOW);
  });

  it('carries forward detail the new event does not mention', () => {
    const task = mergeTask(existing(), incoming({ current: 20 }), NOW);
    expect(task.description).toBe('Found 7 txns');
    expect(task.domainKey).toBe('scan:acc1');
    expect(task.cancelable).toBe(true);
    expect(task.total).toBe(100);
  });

  it('merges meta rather than replacing it', () => {
    const task = mergeTask(existing(), incoming({ meta: { batch: 2 } }), NOW);
    expect(task.meta).toEqual({ account_id: 'acc1', batch: 2 });
  });

  it('lets an explicit null ETA clear a stale estimate', () => {
    expect(mergeTask(existing(), incoming({ etaSeconds: null }), NOW).etaSeconds).toBeNull();
  });

  it('keeps the previous ETA when the event simply omits it', () => {
    expect(mergeTask(existing(), incoming({ current: 20 }), NOW).etaSeconds).toBe(42);
  });

  it('stamps finishedAt the moment the status leaves running', () => {
    expect(mergeTask(existing(), incoming({ status: 'completed' }), NOW).finishedAt).toBe(NOW);
  });

  it('leaves finishedAt alone while the task is still running', () => {
    expect(mergeTask(existing(), incoming({ status: 'running' }), NOW).finishedAt).toBeNull();
  });

  it('keeps a percentage already known when a zero-total event arrives', () => {
    const task = mergeTask(existing({ progressPct: 55 }), incoming({ total: 0, current: 0 }), NOW);
    expect(task.progressPct).toBe(55);
  });

  it('records a failure message and keeps it on later events', () => {
    const failed = mergeTask(existing(), incoming({ status: 'failed', errorMessage: 'oom' }), NOW);
    expect(failed.errorMessage).toBe('oom');
    expect(mergeTask(failed, incoming(), NOW).errorMessage).toBe('oom');
  });
});
