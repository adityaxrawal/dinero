import { describe, it, expect } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import UnassignedInspector from './UnassignedInspector';
import type { UnassignedTransactionRecord } from '@/lib/ipc';

function renderWithQuery(ui: React.ReactElement) {
  const queryClient = new QueryClient();
  return render(<QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>);
}

const baseRecord: UnassignedTransactionRecord = {
  id: 'u1',
  observation_id: 'obs1',
  reason: 'extraction_failed',
  status: 'open',
  created_at: null,
  merchant_raw: 'Google Cloud',
  amount_minor: 3152,
  currency: 'INR',
  direction: null,
  event_time: null,
  source_message_id: null,
  body_snippet: 'You spent Rs 31.52',
  raw_payload_json: null,
};

describe('UnassignedInspector resolve actions', () => {
  it('shows two distinct actions instead of a single Dismiss button', () => {
    renderWithQuery(<UnassignedInspector record={baseRecord} onClose={() => {}} />);
    expect(screen.getByRole('button', { name: /save as transaction/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /not a transaction/i })).toBeInTheDocument();
    expect(screen.queryByText(/dismiss notification/i)).not.toBeInTheDocument();
  });

  it('opens a prefilled form when Save as Transaction is clicked', () => {
    renderWithQuery(<UnassignedInspector record={baseRecord} onClose={() => {}} />);
    fireEvent.click(screen.getByRole('button', { name: /save as transaction/i }));
    expect(screen.getByDisplayValue('Google Cloud')).toBeInTheDocument();
    expect(screen.getByDisplayValue('31.52')).toBeInTheDocument();
  });
});
