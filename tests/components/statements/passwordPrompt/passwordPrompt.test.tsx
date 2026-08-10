// The PDF password prompt: the backend resolves (never throws) for a wrong
// password, so `status` is the only thing distinguishing the outcomes.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import PasswordPromptModal from '@/components/statements/PasswordPromptModal';
import { API } from '@/lib/ipc';
import { useGlobalState } from '@/lib/GlobalStateContext';

vi.mock('@/lib/GlobalStateContext', () => ({ useGlobalState: vi.fn() }));
vi.mock('@/lib/ipc', () => ({
  API: { statements: { listUnprocessed: vi.fn(), submitPassword: vi.fn() } },
}));
vi.mock('@/components/common/GmailEmailViewer', () => ({
  GmailEmailViewer: ({ text }: { text?: string }) => <div>{text ?? 'no body'}</div>,
}));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;
const closePasswordModal = vi.fn();
const watchDraftOrigin = vi.fn();
const openReviewModal = vi.fn();
const onUnlocked = vi.fn();

const entry = (over = {}) => ({
  statement_id: 's1',
  subject: 'Your June statement',
  sender: 'HDFC Alerts <alerts@hdfc.test>',
  date: '2026-06-30T10:00:00Z',
  snippet: 'Password is your DOB',
  ...over,
});

beforeEach(() => {
  vi.clearAllMocks();
  asMock(useGlobalState).mockImplementation(() => ({
    passwordModalOpen: true,
    pendingStatementId: 's1',
    pendingInstrumentId: 'UNKNOWN',
    closePasswordModal,
    watchDraftOrigin,
    openReviewModal,
  }));
  asMock(API.statements.listUnprocessed).mockResolvedValue({ awaiting_password: [entry()] });
});

const render_ = () => render(<PasswordPromptModal onUnlocked={onUnlocked} />);

describe('PasswordPromptModal email context', () => {
  it('shows the bank email so the user can find the stated hint', async () => {
    render_();
    expect(await screen.findByText('Your June statement')).toBeInTheDocument();
    expect(screen.getByText('HDFC Alerts')).toBeInTheDocument();
    expect(screen.getByText('<alerts@hdfc.test>')).toBeInTheDocument();
    expect(screen.getByText('Password is your DOB')).toBeInTheDocument();
  });

  it('falls back to placeholders when the statement is not in the queue', async () => {
    asMock(API.statements.listUnprocessed).mockResolvedValue({ awaiting_password: [] });
    render_();
    expect(await screen.findByText('Statement Context')).toBeInTheDocument();
    expect(screen.getByText('Unknown Sender')).toBeInTheDocument();
  });
});

describe('PasswordPromptModal submission', () => {
  it('keeps Unlock disabled until a password is typed', async () => {
    render_();
    const submit = await screen.findByRole('button', { name: 'Submit PDF password' });
    expect(submit).toBeDisabled();

    fireEvent.change(screen.getByLabelText('PDF Password'), { target: { value: 'hunter2' } });
    expect(submit).toBeEnabled();
  });

  it('opens the review modal directly on the returned draft id', async () => {
    asMock(API.statements.submitPassword).mockResolvedValue({
      status: 'unlocked',
      draft_id: 'draft-9',
    });
    render_();
    fireEvent.change(await screen.findByLabelText('PDF Password'), { target: { value: 'ok' } });
    fireEvent.click(screen.getByRole('button', { name: 'Submit PDF password' }));

    await waitFor(() => expect(openReviewModal).toHaveBeenCalledWith('draft-9'));
    expect(watchDraftOrigin).toHaveBeenCalledWith('s1');
    expect(onUnlocked).toHaveBeenCalled();
  });

  it('reports a wrong password rather than failing silently', async () => {
    asMock(API.statements.submitPassword).mockResolvedValue({ status: 'wrong_password' });
    render_();
    fireEvent.change(await screen.findByLabelText('PDF Password'), { target: { value: 'nope' } });
    fireEvent.click(screen.getByRole('button', { name: 'Submit PDF password' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('Incorrect password');
    expect(openReviewModal).not.toHaveBeenCalled();
  });

  it('closes silently when the instrument gate takes over', async () => {
    asMock(API.statements.submitPassword).mockResolvedValue({
      status: 'awaiting_instrument_confirmation',
    });
    render_();
    fireEvent.change(await screen.findByLabelText('PDF Password'), { target: { value: 'ok' } });
    fireEvent.click(screen.getByRole('button', { name: 'Submit PDF password' }));

    await waitFor(() => expect(closePasswordModal).toHaveBeenCalled());
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(openReviewModal).not.toHaveBeenCalled();
  });

  it('tells the user to re-upload when the unlock session expired', async () => {
    asMock(API.statements.submitPassword).mockRejectedValue('Session has expired');
    render_();
    fireEvent.change(await screen.findByLabelText('PDF Password'), { target: { value: 'ok' } });
    fireEvent.click(screen.getByRole('button', { name: 'Submit PDF password' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('Session expired');
  });

  it('submits on Enter as well as by button', async () => {
    asMock(API.statements.submitPassword).mockResolvedValue({ status: 'unlocked', draft_id: 'd' });
    render_();
    const input = await screen.findByLabelText('PDF Password');
    fireEvent.change(input, { target: { value: 'ok' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => expect(API.statements.submitPassword).toHaveBeenCalled());
  });

  it('toggles password visibility', async () => {
    render_();
    const input = await screen.findByLabelText('PDF Password');
    expect(input).toHaveAttribute('type', 'password');

    fireEvent.click(screen.getByRole('button', { name: 'Show password' }));
    expect(input).toHaveAttribute('type', 'text');
  });

  it('cancels without touching the backend', async () => {
    render_();
    fireEvent.click(await screen.findByRole('button', { name: 'Cancel password entry' }));
    expect(closePasswordModal).toHaveBeenCalled();
    expect(API.statements.submitPassword).not.toHaveBeenCalled();
  });
});
