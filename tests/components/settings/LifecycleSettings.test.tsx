// TASK-DESK-010: both toggles write optimistically and roll back on failure.
// A toggle that reports success while the OS Launch Agent was never written
// is the failure mode worth pinning down.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import LifecycleSettings from '@/components/settings/LifecycleSettings';
import { API } from '@/lib/ipc';

vi.mock('@/lib/ipc', () => ({
  API: {
    lifecycle: {
      getLaunchAtLogin: vi.fn(),
      getBackgroundSyncEnabled: vi.fn(),
      getLowBatteryPollThresholdPercent: vi.fn(),
      setLaunchAtLogin: vi.fn(),
      setBackgroundSyncEnabled: vi.fn(),
      setLowBatteryPollThresholdPercent: vi.fn(),
    },
  },
}));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;
const L = API.lifecycle;

const launchBox = () => screen.getByRole('checkbox', { name: 'Launch Dinero at login' });
const syncBox = () => screen.getByRole('checkbox', { name: 'Continue syncing when app is closed' });
const threshold = () => screen.getByLabelText('Slow down background syncing below');

function setup({ login = false, sync = false, pct = 20 } = {}) {
  asMock(L.getLaunchAtLogin).mockResolvedValue(login);
  asMock(L.getBackgroundSyncEnabled).mockResolvedValue(sync);
  asMock(L.getLowBatteryPollThresholdPercent).mockResolvedValue(pct);
}

beforeEach(() => {
  vi.clearAllMocks();
  setup();
  asMock(L.setLaunchAtLogin).mockResolvedValue(undefined);
  asMock(L.setBackgroundSyncEnabled).mockResolvedValue(undefined);
  asMock(L.setLowBatteryPollThresholdPercent).mockResolvedValue(undefined);
  vi.spyOn(console, 'error').mockImplementation(() => {});
});

afterEach(() => vi.restoreAllMocks());

describe('LifecycleSettings', () => {
  it('hydrates both toggles from the backend', async () => {
    setup({ login: true, sync: true, pct: 35 });
    render(<LifecycleSettings />);

    await waitFor(() => expect(launchBox()).toBeChecked());
    expect(syncBox()).toBeChecked();
    expect(threshold()).toHaveValue(35);
  });

  it('locks both toggles until the initial load settles', () => {
    render(<LifecycleSettings />);

    expect(launchBox()).toBeDisabled();
    expect(syncBox()).toBeDisabled();
  });

  it('persists launch-at-login when toggled', async () => {
    render(<LifecycleSettings />);
    await waitFor(() => expect(launchBox()).toBeEnabled());

    fireEvent.click(launchBox());

    await waitFor(() => expect(L.setLaunchAtLogin).toHaveBeenCalledWith(true));
    expect(launchBox()).toBeChecked();
  });

  it('rolls the launch-at-login toggle back when the write fails', async () => {
    asMock(L.setLaunchAtLogin).mockRejectedValue(new Error('no permission'));
    render(<LifecycleSettings />);
    await waitFor(() => expect(launchBox()).toBeEnabled());

    fireEvent.click(launchBox());

    // Must not stay visually on -- the Launch Agent was never written.
    await waitFor(() => expect(launchBox()).not.toBeChecked());
  });

  it('rolls the background-sync toggle back when the write fails', async () => {
    asMock(L.setBackgroundSyncEnabled).mockRejectedValue(new Error('denied'));
    render(<LifecycleSettings />);
    await waitFor(() => expect(syncBox()).toBeEnabled());

    fireEvent.click(syncBox());

    await waitFor(() => expect(syncBox()).not.toBeChecked());
  });

  it('reveals the low-battery threshold only while background sync is on', async () => {
    render(<LifecycleSettings />);
    await waitFor(() => expect(syncBox()).toBeEnabled());
    expect(screen.queryByLabelText('Slow down background syncing below')).not.toBeInTheDocument();

    fireEvent.click(syncBox());
    await waitFor(() => expect(threshold()).toBeInTheDocument());
  });

  it('persists a changed battery threshold', async () => {
    setup({ sync: true });
    render(<LifecycleSettings />);
    await waitFor(() => expect(threshold()).toBeInTheDocument());

    fireEvent.change(threshold(), { target: { value: '45' } });

    await waitFor(() => expect(L.setLowBatteryPollThresholdPercent).toHaveBeenCalledWith(45));
    expect(threshold()).toHaveValue(45);
  });

  it('still renders with defaults when the initial load fails', async () => {
    asMock(L.getLaunchAtLogin).mockRejectedValue(new Error('ipc down'));
    render(<LifecycleSettings />);

    // Unlocking regardless matters: a failed read must not leave the whole
    // section permanently disabled.
    await waitFor(() => expect(launchBox()).toBeEnabled());
    expect(launchBox()).not.toBeChecked();
  });
});
