import { describe, it, expect, beforeEach, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import AlertBanner from '@/components/notifications/AlertBanner';
import { useAlertStore } from '@/stores/useAlertStore';

vi.mock('@/hooks/use-toast', () => ({ toast: vi.fn() }));

describe('AlertBanner', () => {
  beforeEach(() => {
    useAlertStore.setState({ alerts: [], dismissed: new Set() });
  });

  it('renders nothing when there are no crossed budget alerts', () => {
    render(<AlertBanner />);
    expect(screen.queryByTestId('alert-banner')).toBeNull();
  });

  it('test_banner_persists_until_condition_resolves: shows the highest-severity crossed threshold', () => {
    useAlertStore.getState().onAlertThresholdCrossed({
      transaction_id: 'tx_1',
      alert_type: 'global_budget_80',
      message: 'Global monthly spending at 80% of limit',
    });
    render(<AlertBanner />);
    expect(screen.getByTestId('alert-banner')).toHaveAttribute(
      'data-alert-type',
      'global_budget_80'
    );
    expect(screen.getByText('Global monthly spending at 80% of limit')).toBeTruthy();
  });

  it('prioritizes a 100% (exhausted) crossing over a lower 80% one', () => {
    useAlertStore.getState().onAlertThresholdCrossed({
      transaction_id: 'tx_1',
      alert_type: 'global_budget_80',
      message: 'Global monthly spending at 80% of limit',
    });
    useAlertStore.getState().onAlertThresholdCrossed({
      transaction_id: 'tx_2',
      alert_type: 'category_budget_cat_food_100',
      message: "Category 'cat_food' monthly budget fully exhausted (100%+)",
    });
    render(<AlertBanner />);
    expect(screen.getByTestId('alert-banner')).toHaveAttribute(
      'data-alert-type',
      'category_budget_cat_food_100'
    );
  });

  it('dismissing removes the banner, but a fresh higher crossing brings it back', () => {
    useAlertStore.getState().onAlertThresholdCrossed({
      transaction_id: 'tx_1',
      alert_type: 'global_budget_80',
      message: 'Global monthly spending at 80% of limit',
    });
    const { rerender } = render(<AlertBanner />);
    expect(screen.getByTestId('alert-banner')).toBeTruthy();

    useAlertStore.getState().dismissAlert('global_budget_80');
    rerender(<AlertBanner />);
    expect(screen.queryByTestId('alert-banner')).toBeNull();

    useAlertStore.getState().onAlertThresholdCrossed({
      transaction_id: 'tx_2',
      alert_type: 'global_budget_90',
      message: 'Global monthly spending at 90% of limit',
    });
    rerender(<AlertBanner />);
    expect(screen.getByTestId('alert-banner')).toHaveAttribute(
      'data-alert-type',
      'global_budget_90'
    );
  });
});
