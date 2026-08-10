// TASK-FE-017 / BR-01: the banner must fire not only when no Gmail account is
// connected, but when every connected account has stopped being a working
// ingestion path -- `oauth.rs` keeps a token-refresh-failed account in the
// list as `degraded` rather than removing it, and `polling.rs` only polls
// ACTIVE accounts, so a degraded-only list means sync has silently stopped
// while `connectedAccounts.length` is still > 0.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import StatementOnlyModeBanner from '@/components/shell/StatementOnlyModeBanner';
import { useGlobalState } from '@/lib/GlobalStateContext';

const navigate = vi.fn();
let pathname = '/dashboard';

vi.mock('react-router-dom', () => ({
  useNavigate: () => navigate,
  useLocation: () => ({ pathname }),
}));
vi.mock('@/lib/GlobalStateContext', () => ({ useGlobalState: vi.fn() }));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;

function setAccounts(accounts: Array<{ account_status?: string | null }>) {
  asMock(useGlobalState).mockReturnValue({ connectedAccounts: accounts });
}

const banner = () => screen.queryByRole('status');

beforeEach(() => {
  vi.clearAllMocks();
  pathname = '/dashboard';
  setAccounts([]);
});

describe('StatementOnlyModeBanner', () => {
  it('prompts for statement upload when no account is connected at all', () => {
    setAccounts([]);
    render(<StatementOnlyModeBanner />);

    expect(banner()).toBeInTheDocument();
    expect(screen.getByText(/Gmail sync isn't connected/)).toBeInTheDocument();
  });

  it('stays hidden while at least one account is actively syncing', () => {
    setAccounts([{ account_status: 'ACTIVE' }]);
    render(<StatementOnlyModeBanner />);

    expect(banner()).not.toBeInTheDocument();
  });

  it('treats a degraded-only account list as statement-only mode', () => {
    // The BR-01 case: the row is still present, so a length check misses it.
    setAccounts([{ account_status: 'degraded' }]);
    render(<StatementOnlyModeBanner />);

    expect(banner()).toBeInTheDocument();
  });

  it('matches the active status case-insensitively', () => {
    setAccounts([{ account_status: 'active' }]);
    render(<StatementOnlyModeBanner />);

    expect(banner()).not.toBeInTheDocument();
  });

  it('stays hidden when only one of several accounts still works', () => {
    setAccounts([{ account_status: 'degraded' }, { account_status: 'ACTIVE' }]);
    render(<StatementOnlyModeBanner />);

    expect(banner()).not.toBeInTheDocument();
  });

  it('treats a missing status as not working', () => {
    setAccounts([{ account_status: null }]);
    render(<StatementOnlyModeBanner />);

    expect(banner()).toBeInTheDocument();
  });

  it('says nothing on the Statements route itself', () => {
    // Telling a user already on the upload page to go upload is just noise.
    pathname = '/statements';
    render(<StatementOnlyModeBanner />);

    expect(banner()).not.toBeInTheDocument();
  });

  it('navigates to the upload page', () => {
    render(<StatementOnlyModeBanner />);
    fireEvent.click(screen.getByRole('button', { name: 'Go to statement upload' }));

    expect(navigate).toHaveBeenCalledWith('/statements');
  });

  it('hides on dismiss without persisting the choice', () => {
    render(<StatementOnlyModeBanner />);
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss statement-only mode notice' }));
    expect(banner()).not.toBeInTheDocument();

    // Dismissal is component state, not storage, so a fresh mount shows it again.
    render(<StatementOnlyModeBanner />);
    expect(banner()).toBeInTheDocument();
  });

  it('re-arms the dismissal once Gmail starts working again', () => {
    const { rerender } = render(<StatementOnlyModeBanner />);
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss statement-only mode notice' }));
    expect(banner()).not.toBeInTheDocument();

    setAccounts([{ account_status: 'ACTIVE' }]);
    rerender(<StatementOnlyModeBanner />);

    // Reconnecting clears `dismissed`, so a later re-break shows the banner
    // rather than staying silently suppressed from the earlier click.
    setAccounts([{ account_status: 'degraded' }]);
    rerender(<StatementOnlyModeBanner />);
    expect(banner()).toBeInTheDocument();
  });
});
