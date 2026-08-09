// Covers the License & Billing pane after it was split out of the old
// 797-line Settings.tsx: status grid, state banner, CTAs and the manual
// activation fallback.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import LicenseSection from './LicenseSection';
import { API, type LicenseStatusResponse } from '@/lib/ipc';
import { confirmAction } from '@/lib/confirmDialog';

vi.mock('@/lib/ipc', () => ({
  API: {
    licensing: {
      getStatus: vi.fn(),
      refresh: vi.fn(),
      deactivate: vi.fn(),
      activate: vi.fn(),
      startCheckout: vi.fn(),
    },
  },
}));
vi.mock('@/lib/confirmDialog', () => ({ confirmAction: vi.fn() }));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;

const status = (over: Partial<LicenseStatusResponse> = {}): LicenseStatusResponse =>
  ({
    state: 'ACTIVE',
    is_active: true,
    license_key_masked: null,
    plan_id: 'desktop_pro',
    billing_interval: 'monthly',
    expiry_date: '2027-01-15T00:00:00Z',
    days_remaining: 120,
    ...over,
  }) as LicenseStatusResponse;

/** Renders and waits out the mount-time getStatus round trip. */
async function renderLicense(value: LicenseStatusResponse | null) {
  if (value) asMock(API.licensing.getStatus).mockResolvedValue(value);
  else asMock(API.licensing.getStatus).mockRejectedValue(new Error('offline'));
  render(<LicenseSection />);
  await waitFor(() => expect(screen.queryByText('License & Billing')).toBeInTheDocument());
  await waitFor(() => expect(API.licensing.getStatus).toHaveBeenCalled());
}

beforeEach(() => {
  vi.clearAllMocks();
  asMock(confirmAction).mockResolvedValue(true);
});

describe('LicenseSection status', () => {
  it('reports state, plan, remaining days and renewal date for an active licence', async () => {
    await renderLicense(status());
    expect(await screen.findByText('ACTIVE')).toBeInTheDocument();
    expect(screen.getByText(/desktop_pro/)).toBeInTheDocument();
    expect(screen.getByText('120 Days')).toBeInTheDocument();
    expect(screen.getByText('Renews')).toBeInTheDocument();
  });

  it('labels the countdown as trial time during a trial', async () => {
    await renderLicense(status({ state: 'TRIAL', is_active: false, plan_id: null }));
    expect(await screen.findByText('Trial Left')).toBeInTheDocument();
    expect(screen.queryByText('Plan')).not.toBeInTheDocument();
  });

  it('warns that a locked licence has paid features disabled', async () => {
    await renderLicense(status({ state: 'LOCKED', is_active: false }));
    expect(await screen.findByText(/Your license is locked/)).toBeInTheDocument();
  });

  it('spells out the grace window, pluralised, and hides the generic countdown', async () => {
    await renderLicense(status({ state: 'GRACE', days_remaining: 1 }));
    expect(await screen.findByText(/grace period \(1 day remaining\)/)).toBeInTheDocument();
    expect(screen.queryByText('1 Days')).not.toBeInTheDocument();
  });

  it('says so plainly when the status cannot be loaded', async () => {
    await renderLicense(null);
    expect(await screen.findByText('Could not load license status.')).toBeInTheDocument();
  });
});

describe('LicenseSection actions', () => {
  it('refreshes through the licensing IPC and reloads the status', async () => {
    await renderLicense(status());
    asMock(API.licensing.refresh).mockResolvedValue(undefined);

    fireEvent.click(await screen.findByRole('button', { name: /Refresh License/ }));
    await waitFor(() => expect(API.licensing.refresh).toHaveBeenCalled());
    expect(API.licensing.getStatus).toHaveBeenCalledTimes(2);
  });

  it('surfaces a refresh failure instead of failing silently', async () => {
    await renderLicense(status());
    asMock(API.licensing.refresh).mockRejectedValue(new Error('network down'));

    fireEvent.click(await screen.findByRole('button', { name: /Refresh License/ }));
    expect(await screen.findByText('network down')).toBeInTheDocument();
  });

  it('confirms before deactivating, then calls the IPC', async () => {
    await renderLicense(status());
    asMock(API.licensing.deactivate).mockResolvedValue(undefined);

    fireEvent.click(await screen.findByRole('button', { name: /Deactivate License/ }));
    await waitFor(() => expect(API.licensing.deactivate).toHaveBeenCalled());
  });

  it('does not deactivate when the confirm is declined', async () => {
    asMock(confirmAction).mockResolvedValue(false);
    await renderLicense(status());

    fireEvent.click(await screen.findByRole('button', { name: /Deactivate License/ }));
    await waitFor(() => expect(confirmAction).toHaveBeenCalled());
    expect(API.licensing.deactivate).not.toHaveBeenCalled();
  });

  it('offers Manage Billing on an active plan and no deactivate on an inactive one', async () => {
    await renderLicense(status({ state: 'TRIAL', is_active: false }));
    expect(await screen.findByRole('button', { name: /Subscribe now/ })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Deactivate License/ })).not.toBeInTheDocument();
  });

  it('shows the payment-method CTA during grace', async () => {
    await renderLicense(status({ state: 'GRACE' }));
    expect(
      await screen.findByRole('button', { name: 'Update Payment Method' })
    ).toBeInTheDocument();
  });
});

describe('LicenseSection manual activation fallback', () => {
  it('opens the manual form when Subscribe is used with no email entered', async () => {
    await renderLicense(status({ state: 'TRIAL', is_active: false }));

    fireEvent.click(await screen.findByRole('button', { name: /Subscribe now/ }));
    expect(await screen.findByPlaceholderText('Payment ID')).toBeInTheDocument();
    expect(API.licensing.startCheckout).not.toHaveBeenCalled();
  });

  it('opens hosted checkout, then activates, once an email is present', async () => {
    asMock(API.licensing.startCheckout).mockResolvedValue({
      razorpay_payment_id: 'pay_1',
      razorpay_signature: 'sig_1',
    });
    asMock(API.licensing.activate).mockResolvedValue(undefined);
    await renderLicense(status({ state: 'TRIAL', is_active: false }));

    fireEvent.click(await screen.findByRole('button', { name: /manually/ }));
    fireEvent.change(screen.getByPlaceholderText('Email'), {
      target: { value: 'user@example.com' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Subscribe now/ }));

    await waitFor(() =>
      expect(API.licensing.startCheckout).toHaveBeenCalledWith(
        'user@example.com',
        'desktop_pro_monthly'
      )
    );
    await waitFor(() =>
      expect(API.licensing.activate).toHaveBeenCalledWith(
        'user@example.com',
        'pay_1',
        'sig_1',
        'monthly'
      )
    );
  });

  it('keeps Confirm Activation disabled until every field is filled', async () => {
    await renderLicense(status({ state: 'TRIAL', is_active: false }));
    fireEvent.click(await screen.findByRole('button', { name: /manually/ }));

    const confirm = screen.getByRole('button', { name: 'Confirm Activation' });
    expect(confirm).toBeDisabled();

    fireEvent.change(screen.getByPlaceholderText('Email'), { target: { value: 'a@b.com' } });
    fireEvent.change(screen.getByPlaceholderText('Payment ID'), { target: { value: 'pay_1' } });
    expect(confirm).toBeDisabled();

    fireEvent.change(screen.getByPlaceholderText('Signature'), { target: { value: 'sig_1' } });
    expect(confirm).toBeEnabled();
  });

  it('activates with the trimmed fields and the chosen billing interval', async () => {
    asMock(API.licensing.activate).mockResolvedValue(undefined);
    await renderLicense(status({ state: 'TRIAL', is_active: false }));
    fireEvent.click(await screen.findByRole('button', { name: /manually/ }));

    fireEvent.change(screen.getByPlaceholderText('Email'), { target: { value: ' a@b.com ' } });
    fireEvent.change(screen.getByPlaceholderText('Payment ID'), { target: { value: 'pay_1' } });
    fireEvent.change(screen.getByPlaceholderText('Signature'), { target: { value: 'sig_1' } });
    fireEvent.change(screen.getByRole('combobox'), { target: { value: 'yearly' } });
    fireEvent.click(screen.getByRole('button', { name: 'Confirm Activation' }));

    await waitFor(() =>
      expect(API.licensing.activate).toHaveBeenCalledWith('a@b.com', 'pay_1', 'sig_1', 'yearly')
    );
  });

  it('never renders a card-entry field — checkout stays with Razorpay', async () => {
    await renderLicense(status({ state: 'TRIAL', is_active: false }));
    fireEvent.click(await screen.findByRole('button', { name: /manually/ }));

    for (const label of [/card/i, /cvv/i, /expiry/i]) {
      expect(screen.queryByPlaceholderText(label)).not.toBeInTheDocument();
    }
  });
});
