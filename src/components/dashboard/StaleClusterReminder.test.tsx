import { describe, it, expect } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import StaleClusterReminder, { isClusterStale } from './StaleClusterReminder';
import type { ClusterRecord } from '@/lib/ipc';

function cluster(overrides: Partial<ClusterRecord> = {}): ClusterRecord {
  return {
    id: 'cl_1',
    reason: 'multiple_high_score_candidates',
    members_count: 2,
    members: [],
    created_at: null,
    explanation: 'Two possible matches, close in score.',
    ...overrides,
  };
}

describe('isClusterStale', () => {
  it('test_stale_cluster_reminder_shown_after_7_days: true past the 7-day threshold, false before it', () => {
    const now = new Date('2026-07-20T12:00:00Z');
    // Exactly 7 days ago (or earlier) is stale; a moment more recent isn't.
    expect(isClusterStale('2026-07-13T12:00:01', now)).toBe(false);
    expect(isClusterStale('2026-07-13T12:00:00', now)).toBe(true);
    expect(isClusterStale('2026-07-01 12:00:00', now)).toBe(true); // SQLite space-separated format
  });

  it('a cluster with no created_at is never treated as stale', () => {
    expect(isClusterStale(null)).toBe(false);
  });
});

function renderWithRouter(ui: React.ReactElement) {
  return render(<MemoryRouter>{ui}</MemoryRouter>);
}

describe('StaleClusterReminder', () => {
  it('renders nothing when no cluster is stale', () => {
    renderWithRouter(
      <StaleClusterReminder clusters={[cluster({ created_at: new Date().toISOString() })]} />
    );
    expect(screen.queryByTestId('stale-cluster-reminder')).toBeNull();
  });

  it('shows a count of stale clusters, ignoring fresh ones', () => {
    const eightDaysAgo = new Date(Date.now() - 8 * 24 * 60 * 60 * 1000).toISOString();
    renderWithRouter(
      <StaleClusterReminder
        clusters={[
          cluster({ id: 'cl_stale_1', created_at: eightDaysAgo }),
          cluster({ id: 'cl_stale_2', created_at: eightDaysAgo }),
          cluster({ id: 'cl_fresh', created_at: new Date().toISOString() }),
        ]}
      />
    );
    expect(screen.getByTestId('stale-cluster-reminder')).toBeTruthy();
    expect(screen.getByText(/2 transaction matches still need review/)).toBeTruthy();
  });

  it('is dismissible', () => {
    const eightDaysAgo = new Date(Date.now() - 8 * 24 * 60 * 60 * 1000).toISOString();
    renderWithRouter(<StaleClusterReminder clusters={[cluster({ created_at: eightDaysAgo })]} />);
    expect(screen.getByTestId('stale-cluster-reminder')).toBeTruthy();

    fireEvent.click(screen.getByLabelText('Dismiss stale cluster reminder'));
    expect(screen.queryByTestId('stale-cluster-reminder')).toBeNull();
  });
});
