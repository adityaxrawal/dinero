import { describe, it, expect, beforeEach } from 'vitest';
import { useSyncStore } from './useSyncStore';
import type { ScanProgressPayload } from '@/lib/ipc';

const progress: ScanProgressPayload = {
  account_id: 'acc-1',
  processed: 10,
  total: 100,
  transactions_found: 3,
  statements_found: 1,
  mandate_events_found: 0,
  non_financial: 6,
  errors: 0,
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
