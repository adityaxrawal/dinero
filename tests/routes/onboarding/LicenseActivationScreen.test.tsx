// Covers the license/trial confirmation screen: the three-way hydration
// state and the date formatting, neither of which any other spec renders.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import LicenseActivationScreen from '@/routes/onboarding/LicenseActivationScreen';

const hydrate = vi.fn();
let store: Record<string, unknown> = {};

vi.mock('@/stores/useLicenseStore', () => ({
  useLicenseStore: () => store,
}));

const withStore = (over: Record<string, unknown> = {}) => {
  store = { state: 'TRIAL', hydrated: true, expiryDate: null, hydrate, ...over };
};

beforeEach(() => {
  vi.clearAllMocks();
  withStore();
});

describe('LicenseActivationScreen hydration', () => {
  it('hydrates the license state on mount', () => {
    render(<LicenseActivationScreen onContinue={vi.fn()} />);
    expect(hydrate).toHaveBeenCalledTimes(1);
  });

  it('shows a checking message until the store has hydrated', () => {
    withStore({ hydrated: false });
    render(<LicenseActivationScreen onContinue={vi.fn()} />);
    expect(screen.getByText(/Checking your license status/)).toBeInTheDocument();
    expect(screen.queryByText(/free trial has started/)).not.toBeInTheDocument();
  });

  it.each(['ACTIVE', 'GRACE'])('treats %s as an existing subscription', (state) => {
    withStore({ state });
    render(<LicenseActivationScreen onContinue={vi.fn()} />);
    expect(screen.getByText(/already have an active subscription/)).toBeInTheDocument();
    // The Settings upsell is for unpaid users only.
    expect(screen.queryByText(/Already have a subscription\?/)).not.toBeInTheDocument();
  });

  it('announces the trial and points unpaid users at Settings', () => {
    render(<LicenseActivationScreen onContinue={vi.fn()} />);
    expect(screen.getByText(/14-day free trial has started/)).toBeInTheDocument();
    expect(screen.getByText(/Already have a subscription\?/)).toBeInTheDocument();
  });
});

describe('LicenseActivationScreen trial expiry', () => {
  it('names the end date when the store knows it', () => {
    withStore({ expiryDate: '2026-08-23T00:00:00Z' });
    render(<LicenseActivationScreen onContinue={vi.fn()} />);
    expect(screen.getByText(/Trial ends on/)).toBeInTheDocument();
    expect(screen.getByText(/2026/)).toBeInTheDocument();
  });

  it('falls back to the generic line when no expiry is known', () => {
    render(<LicenseActivationScreen onContinue={vi.fn()} />);
    expect(screen.queryByText(/Trial ends on/)).not.toBeInTheDocument();
    expect(screen.getByText('No credit card required.')).toBeInTheDocument();
  });

  it('never renders "Invalid Date" for an unparseable expiry', () => {
    withStore({ expiryDate: 'not-a-real-date' });
    render(<LicenseActivationScreen onContinue={vi.fn()} />);
    expect(screen.queryByText(/Invalid Date/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Trial ends on/)).not.toBeInTheDocument();
    expect(screen.getByText('No credit card required.')).toBeInTheDocument();
  });
});

describe('LicenseActivationScreen actions', () => {
  it('hands control back to the caller on continue', () => {
    const onContinue = vi.fn();
    render(<LicenseActivationScreen onContinue={onContinue} />);
    fireEvent.click(screen.getByRole('button', { name: 'Continue to the dashboard' }));
    expect(onContinue).toHaveBeenCalledTimes(1);
  });
});
