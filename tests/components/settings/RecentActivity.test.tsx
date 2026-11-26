// Scan and import outcomes persist across restarts (audit F5-4).
import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import RecentActivity from '@/components/settings/RecentActivity';

vi.mock('@/lib/ipc', () => ({
  API: {
    activity: {
      list: () =>
        Promise.resolve([
          { id: '1', kind: 'scan', outcome: 'completed', summary: 'Scan completed: 258 emails checked, 34 booked', details_json: null, created_at: '2026-11-25 20:37:34' },
          { id: '2', kind: 'scan', outcome: 'failed', summary: 'Scan failed: network', details_json: null, created_at: null },
        ]),
    },
  },
}));

describe('RecentActivity', () => {
  it('lists recorded scan outcomes', async () => {
    render(
      <QueryClientProvider client={new QueryClient()}>
        <RecentActivity />
      </QueryClientProvider>
    );
    expect(await screen.findByText(/258 emails checked, 34 booked/)).toBeInTheDocument();
    expect(screen.getByText('Scan failed: network')).toBeInTheDocument();
  });
});
