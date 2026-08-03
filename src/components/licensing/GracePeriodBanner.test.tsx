import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import GracePeriodBanner, { isGraceUrgent } from './GracePeriodBanner';
import { useLicenseStore } from '@/stores/useLicenseStore';

const handleRetry = vi.fn();
const openUrl = vi.fn();
let isRetrying = false;

vi.mock('@/hooks/useLicenseRefresh', () => ({
  useLicenseRefresh: () => ({ isRetrying, handleRetry }),
}));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: (...a: unknown[]) => openUrl(...a) }));

// Doc 30 TASK-BILL-004: "Day 1-3 informational amber; Day 4-7 prominent
// red." daysRemaining counts down from 7, so <=3 remaining means >=4 days
// have elapsed.
describe('isGraceUrgent', () => {
  it('is not urgent with 7/6/5/4 days remaining (Day 1-3 elapsed)', () => {
    expect(isGraceUrgent(7)).toBe(false);
    expect(isGraceUrgent(6)).toBe(false);
    expect(isGraceUrgent(5)).toBe(false);
    expect(isGraceUrgent(4)).toBe(false);
  });

  it('is urgent with 3/2/1/0 days remaining (Day 4-7 elapsed)', () => {
    expect(isGraceUrgent(3)).toBe(true);
    expect(isGraceUrgent(2)).toBe(true);
    expect(isGraceUrgent(1)).toBe(true);
    expect(isGraceUrgent(0)).toBe(true);
  });

  it('is not urgent when days remaining is unknown', () => {
    expect(isGraceUrgent(null)).toBe(false);
  });
});

const setLicense = (state: string, daysRemainingInTrial: number | null) =>
  useLicenseStore.setState({ state, daysRemainingInTrial } as never);

describe('GracePeriodBanner', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    isRetrying = false;
    setLicense('GRACE', 5);
  });

  it.each(['ACTIVE', 'TRIAL', 'EXPIRED', 'LOCKED'])('renders nothing in %s state', (state) => {
    setLicense(state, 5);
    const { container } = render(<GracePeriodBanner />);
    expect(container).toBeEmptyDOMElement();
  });

  it('warns while in the grace period', () => {
    render(<GracePeriodBanner />);
    expect(screen.getByRole('status')).toBeTruthy();
    expect(screen.getByText(/Subscription in grace period/)).toBeTruthy();
  });

  it('counts down the days remaining', () => {
    render(<GracePeriodBanner />);
    expect(screen.getByText(/5 days left/)).toBeTruthy();
  });

  it('uses the singular form on the last day', () => {
    setLicense('GRACE', 1);
    render(<GracePeriodBanner />);
    expect(screen.getByText(/1 day left/)).toBeTruthy();
  });

  it('omits the countdown when the remaining days are unknown', () => {
    setLicense('GRACE', null);
    render(<GracePeriodBanner />);
    expect(screen.queryByText(/left/)).toBeNull();
    expect(screen.getByText(/Resolve payment/)).toBeTruthy();
  });

  it('can be dismissed', () => {
    render(<GracePeriodBanner />);
    fireEvent.click(screen.getByLabelText('Dismiss grace period notice'));
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('comes back after leaving and re-entering the grace period', () => {
    const { rerender } = render(<GracePeriodBanner />);
    fireEvent.click(screen.getByLabelText('Dismiss grace period notice'));
    setLicense('ACTIVE', null);
    rerender(<GracePeriodBanner />);
    setLicense('GRACE', 2);
    rerender(<GracePeriodBanner />);
    expect(screen.getByRole('status')).toBeTruthy();
  });

  it('always offers a retry', () => {
    render(<GracePeriodBanner />);
    fireEvent.click(screen.getByLabelText('Retry validation now'));
    expect(handleRetry).toHaveBeenCalled();
  });

  it('disables retry while one is in flight', () => {
    isRetrying = true;
    render(<GracePeriodBanner />);
    expect(screen.getByLabelText('Retry validation now')).toHaveProperty('disabled', true);
  });

  describe('urgency escalation', () => {
    it('offers no payment-portal shortcut while there is still time', () => {
      setLicense('GRACE', 5);
      render(<GracePeriodBanner />);
      expect(screen.queryByLabelText('Update payment method')).toBeNull();
    });

    it('offers the payment-portal shortcut in the final days', () => {
      setLicense('GRACE', 2);
      render(<GracePeriodBanner />);
      expect(screen.getByLabelText('Update payment method')).toBeTruthy();
    });

    it('opens the portal in the system browser, not the app webview', async () => {
      setLicense('GRACE', 2);
      render(<GracePeriodBanner />);
      fireEvent.click(screen.getByLabelText('Update payment method'));
      await waitFor(() =>
        expect(openUrl).toHaveBeenCalledWith(expect.stringContaining('razorpay.com'))
      );
    });

    it('switches to the red palette when urgent', () => {
      setLicense('GRACE', 2);
      const { container } = render(<GracePeriodBanner />);
      expect(container.firstElementChild!.className).toContain('red');
    });

    it('uses the amber palette while not urgent', () => {
      setLicense('GRACE', 10);
      const { container } = render(<GracePeriodBanner />);
      expect(container.firstElementChild!.className).toContain('amber');
    });
  });
});
