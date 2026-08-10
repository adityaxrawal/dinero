import { describe, it, expect, beforeEach } from 'vitest';
import { useSyncStore } from '@/stores/useSyncStore';
import type { ScanProgressPayload, ScanStatusResponse } from '@/lib/ipc';

const progress: ScanProgressPayload = {
  account_id: 'acc-1',
  processed: 10,
  total: 100,
  transactions_found: 3,
  statements_found: 1,
  mandate_events_found: 0,
  non_financial: 6,
  errors: 0,
  pending_enrichment: 0,
};

describe('useSyncStore', () => {
  beforeEach(() => {
    useSyncStore.setState({
      scanStatus: 'idle',
      scanProgress: null,
      scanError: null,
      warnings: [],
    });
  });

  it('moves to running on scan_progress and clears any prior error', () => {
    useSyncStore.setState({ scanStatus: 'error', scanError: 'boom' });
    useSyncStore.getState().onScanProgress(progress);
    expect(useSyncStore.getState().scanStatus).toBe('running');
    expect(useSyncStore.getState().scanProgress).toEqual(progress);
    expect(useSyncStore.getState().scanError).toBeNull();
  });

  it('moves to done on scan_completed', () => {
    useSyncStore.getState().onScanProgress(progress);
    useSyncStore.getState().onScanCompleted();
    expect(useSyncStore.getState().scanStatus).toBe('done');
  });

  it('moves to error with message on scan_failed', () => {
    useSyncStore.getState().onScanFailed('Gmail API rate limited');
    expect(useSyncStore.getState().scanStatus).toBe('error');
    expect(useSyncStore.getState().scanError).toBe('Gmail API rate limited');
  });

  it('accumulates and dismisses system warnings independently', () => {
    useSyncStore.getState().onSystemWarning({ warning_type: 'low_ram', message: 'Low RAM' });
    useSyncStore.getState().onSystemWarning({ warning_type: 'db_size', message: 'DB large' });
    expect(useSyncStore.getState().warnings).toHaveLength(2);

    useSyncStore.getState().dismissWarning(0);
    expect(useSyncStore.getState().warnings).toHaveLength(1);
    expect(useSyncStore.getState().warnings[0].warning_type).toBe('db_size');
  });

  it('resetScanState returns to idle with no progress/error', () => {
    useSyncStore.getState().onScanProgress(progress);
    useSyncStore.getState().resetScanState();
    expect(useSyncStore.getState().scanStatus).toBe('idle');
    expect(useSyncStore.getState().scanProgress).toBeNull();
    expect(useSyncStore.getState().scanError).toBeNull();
  });
});

/**
 * audit_07 #7: a historical scan runs in the backend regardless of the
 * webview, so reloading mid-scan used to leave the UI showing no scan at all
 * until the next `scan_progress` event. `hydrateScanState` seeds from the
 * persisted checkpoint instead — but it must never rewind a scan whose live
 * events have already started arriving, since the checkpoint is only written
 * every CHECKPOINT_INTERVAL messages and is therefore always the older number.
 */
describe('useSyncStore scan re-hydration', () => {
  const inProgress: ScanStatusResponse = {
    status: 'in_progress',
    processed: 40,
    total: 100,
    transactions_found: 12,
    statements_found: 2,
    mandate_events_found: 1,
    errors: 0,
    pending_enrichment: 3,
  };

  beforeEach(() => {
    useSyncStore.setState({
      scanStatus: 'idle',
      scanProgress: null,
      scanError: null,
      warnings: [],
    });
  });

  it('seeds a running scan from the checkpoint when nothing is known yet', () => {
    useSyncStore.getState().hydrateScanState('acc-1', inProgress);

    const s = useSyncStore.getState();
    expect(s.scanStatus).toBe('running');
    expect(s.scanProgress).toMatchObject({
      account_id: 'acc-1',
      processed: 40,
      total: 100,
      transactions_found: 12,
      pending_enrichment: 3,
    });
  });

  it('does not rewind a scan that already has live progress', () => {
    useSyncStore.getState().onScanProgress({ ...progress, processed: 90 });
    useSyncStore.getState().hydrateScanState('acc-1', inProgress);

    expect(useSyncStore.getState().scanProgress?.processed).toBe(90);
  });

  it('ignores checkpoints for scans that are not running', () => {
    for (const status of ['not_started', 'completed', 'failed', 'cancelled', 'paused']) {
      useSyncStore.setState({ scanStatus: 'idle', scanProgress: null, scanError: null });
      useSyncStore.getState().hydrateScanState('acc-1', { ...inProgress, status });
      expect(useSyncStore.getState().scanStatus).toBe('idle');
      expect(useSyncStore.getState().scanProgress).toBeNull();
    }
  });

  it('does not clobber a completed scan the user is still looking at', () => {
    useSyncStore.getState().onScanCompleted();
    useSyncStore.getState().hydrateScanState('acc-1', inProgress);

    expect(useSyncStore.getState().scanStatus).toBe('done');
  });
});
