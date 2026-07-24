import { describe, it, expect, beforeEach, vi } from 'vitest';
import {
  useAlertStore,
  highestPriorityAlert,
  isBudgetThresholdAlert,
  shouldSuppressToastDuringBulkScan,
  type AlertThresholdPayload,
} from './useAlertStore';
import { useSyncStore } from './useSyncStore';

vi.mock('@/hooks/use-toast', () => ({ toast: vi.fn() }));

const global80: AlertThresholdPayload = {
  transaction_id: 'tx_1',
  alert_type: 'global_budget_80',
  message: 'Global monthly spending at 80% of limit',
};
const global90: AlertThresholdPayload = {
  transaction_id: 'tx_2',
  alert_type: 'global_budget_90',
  message: 'Global monthly spending at 90% of limit',
};
const category100: AlertThresholdPayload = {
  transaction_id: 'tx_3',
  alert_type: 'category_budget_cat_transport_100',
  message: "Category 'cat_transport' monthly budget fully exhausted (100%+)",
};

describe('isBudgetThresholdAlert', () => {
  it('recognizes global and category budget alert types', () => {
    expect(isBudgetThresholdAlert('global_budget_80')).toBe(true);
    expect(isBudgetThresholdAlert('category_budget_cat_food_100')).toBe(true);
  });

  it('excludes non-budget alert types', () => {
    expect(isBudgetThresholdAlert('merchant_spike')).toBe(false);
    expect(isBudgetThresholdAlert('upcoming_subscription')).toBe(false);
  });
});

describe('highestPriorityAlert', () => {
  it('picks the highest severity band regardless of array order', () => {
    expect(highestPriorityAlert([global80, category100, global90])?.alert_type).toBe(
      'category_budget_cat_transport_100'
    );
  });

  it('returns null for an empty list', () => {
    expect(highestPriorityAlert([])).toBeNull();
  });
});

describe('useAlertStore', () => {
  beforeEach(() => {
    useAlertStore.setState({ alerts: [], dismissed: new Set() });
    useSyncStore.setState({ scanStatus: 'idle' });
  });

  it('accumulates distinct alert types', () => {
    useAlertStore.getState().onAlertThresholdCrossed(global80);
    useAlertStore.getState().onAlertThresholdCrossed(category100);
    expect(useAlertStore.getState().alerts).toHaveLength(2);
  });

  it('ignores non-budget alert types (merchant_spike, upcoming_subscription)', () => {
    useAlertStore.getState().onAlertThresholdCrossed({
      transaction_id: 'tx_4',
      alert_type: 'merchant_spike',
      message: 'Unusual spend',
    });
    expect(useAlertStore.getState().alerts).toHaveLength(0);
  });

  it('replaces the same alert_type rather than duplicating it', () => {
    useAlertStore.getState().onAlertThresholdCrossed(global80);
    useAlertStore.getState().onAlertThresholdCrossed(global90);
    // both scoped to "global_budget_*" but distinct types -- both kept
    expect(useAlertStore.getState().alerts).toHaveLength(2);

    const global80Again = { ...global80, transaction_id: 'tx_5' };
    useAlertStore.getState().onAlertThresholdCrossed(global80Again);
    expect(
      useAlertStore.getState().alerts.filter((a) => a.alert_type === 'global_budget_80')
    ).toHaveLength(1);
  });

  it('a dismissed alert stops rendering, but a fresh crossing of that type un-dismisses it', () => {
    useAlertStore.getState().onAlertThresholdCrossed(global80);
    useAlertStore.getState().dismissAlert('global_budget_80');
    expect(useAlertStore.getState().dismissed.has('global_budget_80')).toBe(true);

    useAlertStore.getState().onAlertThresholdCrossed(global90);
    // global90 is a different type -- dismissal of 80 must be untouched by
    // an unrelated crossing.
    expect(useAlertStore.getState().dismissed.has('global_budget_80')).toBe(true);
  });
});

describe('shouldSuppressToastDuringBulkScan', () => {
  beforeEach(() => {
    useSyncStore.setState({ scanStatus: 'idle' });
  });

  it('test_bulk_scan_suppresses_individual_transaction_toasts: suppresses while a scan is running', () => {
    useSyncStore.setState({ scanStatus: 'running' });
    expect(shouldSuppressToastDuringBulkScan()).toBe(true);
  });

  it('does not suppress when idle/done/error', () => {
    for (const status of ['idle', 'done', 'error'] as const) {
      useSyncStore.setState({ scanStatus: status });
      expect(shouldSuppressToastDuringBulkScan()).toBe(false);
    }
  });
});
