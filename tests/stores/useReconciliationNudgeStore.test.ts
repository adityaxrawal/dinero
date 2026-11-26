import { describe, it, expect, beforeEach } from 'vitest';
import { useReconciliationNudgeStore } from '@/stores/useReconciliationNudgeStore';
import { useNotificationStore } from '@/stores/useNotificationStore';

/** Puts a scan task in the notification store (the scan-running source of truth). */
const setScan = (status: 'idle' | 'running' | 'completed' | 'failed' | 'cancelled') =>
  useNotificationStore.setState({
    tasks:
      status === 'idle'
        ? {}
        : ({
            'scan:acc': { id: 'scan:acc', domainKey: 'scan:acc', status },
          } as unknown as ReturnType<typeof useNotificationStore.getState>['tasks']),
  });

describe('useReconciliationNudgeStore', () => {
  beforeEach(() => {
    useReconciliationNudgeStore.setState({ justPulsed: false, pendingSinceLastPulse: 0 });
    setScan('idle');
  });

  it('pulses immediately for a single cluster created outside a scan', () => {
    useReconciliationNudgeStore.getState().onClusterCreated();
    expect(useReconciliationNudgeStore.getState().justPulsed).toBe(true);
  });

  it('test_burst_creation_during_scan_suppresses_individual_nudges: no pulse fires for any individual cluster while a scan is running', () => {
    setScan('running');

    for (let i = 0; i < 5; i++) {
      useReconciliationNudgeStore.getState().onClusterCreated();
      expect(useReconciliationNudgeStore.getState().justPulsed).toBe(false);
    }
    expect(useReconciliationNudgeStore.getState().pendingSinceLastPulse).toBe(5);
  });

  it('fires a single aggregate pulse on scan_completed for clusters that arrived mid-scan', () => {
    setScan('running');
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
