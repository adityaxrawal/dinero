import { describe, it, expect, beforeEach } from 'vitest';
import { useReconciliationNudgeStore } from '@/stores/useReconciliationNudgeStore';
import { useSyncStore } from '@/stores/useSyncStore';

describe('useReconciliationNudgeStore', () => {
  beforeEach(() => {
    useReconciliationNudgeStore.setState({ justPulsed: false, pendingSinceLastPulse: 0 });
    useSyncStore.setState({ scanStatus: 'idle' });
  });

  it('pulses immediately for a single cluster created outside a scan', () => {
    useReconciliationNudgeStore.getState().onClusterCreated();
    expect(useReconciliationNudgeStore.getState().justPulsed).toBe(true);
  });

  it('test_burst_creation_during_scan_suppresses_individual_nudges: no pulse fires for any individual cluster while a scan is running', () => {
    useSyncStore.setState({ scanStatus: 'running' });

    for (let i = 0; i < 5; i++) {
      useReconciliationNudgeStore.getState().onClusterCreated();
      expect(useReconciliationNudgeStore.getState().justPulsed).toBe(false);
    }
    expect(useReconciliationNudgeStore.getState().pendingSinceLastPulse).toBe(5);
  });

  it('fires a single aggregate pulse on scan_completed for clusters that arrived mid-scan', () => {
    useSyncStore.setState({ scanStatus: 'running' });
    useReconciliationNudgeStore.getState().onClusterCreated();
    useReconciliationNudgeStore.getState().onClusterCreated();
    expect(useReconciliationNudgeStore.getState().justPulsed).toBe(false);

    useReconciliationNudgeStore.getState().onScanCompleted();
    expect(useReconciliationNudgeStore.getState().justPulsed).toBe(true);
    expect(useReconciliationNudgeStore.getState().pendingSinceLastPulse).toBe(0);
  });

  it('scan_completed with nothing pending is a no-op', () => {
    useReconciliationNudgeStore.getState().onScanCompleted();
    expect(useReconciliationNudgeStore.getState().justPulsed).toBe(false);
  });

  it('clearPulse resets the pulse flag', () => {
    useReconciliationNudgeStore.getState().onClusterCreated();
    expect(useReconciliationNudgeStore.getState().justPulsed).toBe(true);
    useReconciliationNudgeStore.getState().clearPulse();
    expect(useReconciliationNudgeStore.getState().justPulsed).toBe(false);
  });
});
