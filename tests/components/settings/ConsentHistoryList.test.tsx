import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import ConsentHistoryList from '@/components/settings/ConsentHistoryList';
import type { ConsentEventRecord } from '@/lib/ipc';

const onRefresh = vi.fn();

const event = (over: Partial<ConsentEventRecord> = {}): ConsentEventRecord =>
  ({
    id: 'evt1',
    event_type: 'gmail_connected',
    disclosure_text: 'Read-only access to your Gmail messages.',
    consented_at: '2026-03-04T10:15:00Z',
    withdrawn_at: null,
    ...over,
  }) as ConsentEventRecord;

function renderList(over: Record<string, unknown> = {}) {
  return render(
    <ConsentHistoryList events={[]} isLoading={false} onRefresh={onRefresh} {...over} />
  );
}

beforeEach(() => vi.clearAllMocks());

describe('ConsentHistoryList', () => {
  it('says so when nothing has been recorded', () => {
    renderList();
    expect(screen.getByText('No consent events recorded yet.')).toBeInTheDocument();
  });

  it('shows the loading state instead of the empty state', () => {
    // Both are "no events on screen" -- an in-flight load must not read as
    // "you have never granted consent".
    renderList({ isLoading: true });

    expect(screen.getByText('Loading…')).toBeInTheDocument();
    expect(screen.queryByText('No consent events recorded yet.')).not.toBeInTheDocument();
  });

  it('renders each event with its type and disclosure', () => {
    renderList({ events: [event(), event({ id: 'evt2', event_type: 'gmail_revoked' })] });

    expect(screen.getByText('gmail_connected')).toBeInTheDocument();
    expect(screen.getByText('gmail_revoked')).toBeInTheDocument();
    expect(screen.getAllByText('Read-only access to your Gmail messages.')).toHaveLength(2);
  });

  it('marks withdrawal only on events that were withdrawn', () => {
    renderList({
      events: [event(), event({ id: 'evt2', withdrawn_at: '2026-05-01T09:00:00Z' })],
    });

    expect(screen.getAllByText(/^Withdrawn /)).toHaveLength(1);
  });

  it('refreshes on demand and locks the button while loading', () => {
    renderList();
    const button = screen.getByRole('button', { name: 'Refresh consent history' });

    fireEvent.click(button);
    expect(onRefresh).toHaveBeenCalledTimes(1);

    renderList({ isLoading: true });
    expect(screen.getAllByRole('button', { name: 'Refresh consent history' })[1]).toBeDisabled();
  });
});
