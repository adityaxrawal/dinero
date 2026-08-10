import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import ConnectedAccountsSettings from '@/components/settings/ConnectedAccountsSettings';
import { API } from '@/lib/ipc';
import { useGlobalState } from '@/lib/GlobalStateContext';

let connectedAccounts: Array<Record<string, unknown>> = [];
const refreshConnectedAccounts = vi.fn();

vi.mock('@/lib/GlobalStateContext', () => ({ useGlobalState: vi.fn() }));
vi.mock('@/lib/ipc', () => ({
  API: { auth: { startGoogle: vi.fn(), revokeGoogle: vi.fn() } },
}));
vi.mock('@/components/settings/RevokeGmailButton', () => ({
  default: ({ onRevoke }: { onRevoke: () => void }) => (
    <button onClick={onRevoke}>Disconnect</button>
  ),
}));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;

const account = (over = {}) => ({
  account_id: 'acc1',
  email: 'user@gmail.com',
  account_status: 'active',
  ...over,
});

beforeEach(() => {
  vi.clearAllMocks();
  connectedAccounts = [];
  asMock(useGlobalState).mockImplementation(() => ({
    connectedAccounts,
    refreshConnectedAccounts,
  }));
  asMock(API.auth.startGoogle).mockResolvedValue(undefined);
});

describe('ConnectedAccountsSettings', () => {
  it('invites a first connection when none exist', () => {
    render(<ConnectedAccountsSettings />);
    expect(screen.getByText('Connect Gmail')).toBeInTheDocument();
  });

  it('offers to add another once one is connected', () => {
    connectedAccounts = [account()];
    render(<ConnectedAccountsSettings />);
    expect(screen.getByText('Connect Another Gmail Account')).toBeInTheDocument();
  });

  it('lists each connected account', () => {
    connectedAccounts = [account(), account({ account_id: 'acc2', email: 'two@gmail.com' })];
    render(<ConnectedAccountsSettings />);
    expect(screen.getByText('user@gmail.com')).toBeInTheDocument();
    expect(screen.getByText('two@gmail.com')).toBeInTheDocument();
  });

  it('shows a healthy account as connected', () => {
    connectedAccounts = [account()];
    render(<ConnectedAccountsSettings />);
    expect(screen.getByText('Gmail Connected')).toBeInTheDocument();
    expect(screen.queryByText('Reconnect')).toBeNull();
  });

  it('surfaces a degraded account with a reconnect action', () => {
    connectedAccounts = [account({ account_status: 'degraded' })];
    render(<ConnectedAccountsSettings />);
    expect(screen.getByText('Needs Reconnection')).toBeInTheDocument();
    expect(screen.getByText('Reconnect')).toBeInTheDocument();
  });

  it('matches the degraded status case-insensitively', () => {
    connectedAccounts = [account({ account_status: 'DEGRADED' })];
    render(<ConnectedAccountsSettings />);
    expect(screen.getByText('Needs Reconnection')).toBeInTheDocument();
  });

  it('starts the OAuth flow and refreshes the list', async () => {
    render(<ConnectedAccountsSettings />);
    fireEvent.click(screen.getByText('Connect Gmail'));
    await waitFor(() => expect(API.auth.startGoogle).toHaveBeenCalled());
    await waitFor(() => expect(refreshConnectedAccounts).toHaveBeenCalled());
  });

  it('surfaces a connection failure', async () => {
    asMock(API.auth.startGoogle).mockRejectedValue(new Error('oauth cancelled'));
    render(<ConnectedAccountsSettings />);
    fireEvent.click(screen.getByText('Connect Gmail'));
    expect(await screen.findByText(/oauth cancelled/)).toBeInTheDocument();
  });

  it('withdraws the connect button at the ten-account cap', () => {
    connectedAccounts = Array.from({ length: 10 }, (_, i) =>
      account({ account_id: `acc${i}`, email: `u${i}@gmail.com` })
    );
    render(<ConnectedAccountsSettings />);
    expect(screen.queryByText('Connect Another Gmail Account')).toBeNull();
  });
});
