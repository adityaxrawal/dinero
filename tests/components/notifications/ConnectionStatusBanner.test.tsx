import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import ConnectionStatusBanner from '@/components/notifications/ConnectionStatusBanner';
import { API } from '@/lib/ipc';

vi.mock('@/lib/ipc', async () => {
  const actual = await vi.importActual<typeof import('@/lib/ipc')>('@/lib/ipc');
  return {
    ...actual,
    API: {
      systemWarnings: {
        getActive: vi.fn(),
      },
    },
  };
});

const listenHandlers: Record<string, (payload: unknown) => void> = {};
vi.mock('@/hooks/useIpcListen', () => ({
  useIpcListen: (event: string, handler: (payload: unknown) => void) => {
    listenHandlers[event] = handler;
  },
}));

describe('ConnectionStatusBanner', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    for (const k of Object.keys(listenHandlers)) delete listenHandlers[k];
  });

  it('renders nothing when there are no active warnings', async () => {
    vi.mocked(API.systemWarnings.getActive).mockResolvedValue([]);
    render(<ConnectionStatusBanner />);
    await waitFor(() => expect(API.systemWarnings.getActive).toHaveBeenCalled());
    expect(screen.queryByTestId('connection-status-banner')).toBeNull();
  });

  it('shows a late-mount warning fetched from get_active_system_warnings', async () => {
    vi.mocked(API.systemWarnings.getActive).mockResolvedValue([
      { warning_type: 'low_ram', message: 'Low RAM detected', severity: 'info', action_hint: null },
    ]);
    render(<ConnectionStatusBanner />);
    await waitFor(() => {
      expect(screen.getByTestId('connection-status-banner')).toHaveAttribute(
        'data-warning-type',
        'low_ram'
      );
    });
    expect(screen.getByText('Low RAM detected')).toBeTruthy();
  });

  it('prioritizes the highest-severity warning when multiple are active', async () => {
    vi.mocked(API.systemWarnings.getActive).mockResolvedValue([
      { warning_type: 'low_ram', message: 'Low RAM', severity: 'info', action_hint: null },
      {
        warning_type: 'gmail_quota_exhausted',
        message: 'Gmail paused',
        severity: 'degraded',
        action_hint: null,
      },
      {
        warning_type: 'clock_skew',
        message: 'Clock skew',
        severity: 'critical',
        action_hint: 'check_system_clock',
      },
    ]);
    render(<ConnectionStatusBanner />);
    await waitFor(() => {
      expect(screen.getByTestId('connection-status-banner')).toHaveAttribute(
        'data-warning-type',
        'clock_skew'
      );
    });
  });

  it('excludes keychain_denied/notification_denied -- owned by PermissionDeniedOverlay', async () => {
    vi.mocked(API.systemWarnings.getActive).mockResolvedValue([
      {
        warning_type: 'keychain_denied',
        message: 'Keychain',
        severity: 'critical',
        action_hint: null,
      },
    ]);
    render(<ConnectionStatusBanner />);
    await waitFor(() => expect(API.systemWarnings.getActive).toHaveBeenCalled());
    expect(screen.queryByTestId('connection-status-banner')).toBeNull();
  });

  it('reacts to a live system_warning event', async () => {
    vi.mocked(API.systemWarnings.getActive).mockResolvedValue([]);
    render(<ConnectionStatusBanner />);
    await waitFor(() => expect(listenHandlers['system_warning']).toBeDefined());

    listenHandlers['system_warning']({
      warning_type: 'gmail_token_degraded',
      message: 'Gmail token degraded',
      severity: 'degraded',
      action_hint: 'reconnect_gmail_account',
    });

    await waitFor(() => {
      expect(screen.getByTestId('connection-status-banner')).toHaveAttribute(
        'data-warning-type',
        'gmail_token_degraded'
      );
    });
  });

  it('clears a warning on system_warning_cleared', async () => {
    vi.mocked(API.systemWarnings.getActive).mockResolvedValue([
      { warning_type: 'low_ram', message: 'Low RAM', severity: 'info', action_hint: null },
    ]);
    render(<ConnectionStatusBanner />);
    await waitFor(() => expect(screen.getByTestId('connection-status-banner')).toBeTruthy());

    listenHandlers['system_warning_cleared']('low_ram');

    await waitFor(() => expect(screen.queryByTestId('connection-status-banner')).toBeNull());
  });
});
