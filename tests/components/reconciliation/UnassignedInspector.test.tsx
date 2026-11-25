import { describe, it, expect, vi } from 'vitest';
import { waitFor } from '@testing-library/react';
import { render, screen, fireEvent } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import UnassignedInspector from '@/components/reconciliation/UnassignedInspector';
import { API, type UnassignedTransactionRecord } from '@/lib/ipc';

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

  it('renders ingestion diagnostic checklist and confidence level metrics', () => {
    const recordWithConfidence: UnassignedTransactionRecord = {
      ...baseRecord,
      reason: 'issuer_name_not_found',
      extraction_method: 'llm_layer6',
      confidence_score: 0.75,
      event_time: '2026-07-30T10:00:00Z',
    };

    renderWithQuery(<UnassignedInspector record={recordWithConfidence} onClose={() => {}} />);

    expect(screen.getByText('Unknown Payment Instrument')).toBeInTheDocument();
    expect(screen.getByText('75% Confidence')).toBeInTheDocument();
    expect(screen.getByText('AI Layer 6 LLM')).toBeInTheDocument();
    expect(screen.getByText('Ingestion Diagnostic Checklist')).toBeInTheDocument();
    expect(screen.getByText('Card/Account Unmatched in Settings')).toBeInTheDocument();
  });
});

describe('UnassignedInspector re-check (current-issues #3)', () => {
  it('disables Re-check for an entry with no stored email', () => {
    renderWithQuery(<UnassignedInspector record={baseRecord} onClose={() => {}} />);
    const button = screen.getByRole('button', { name: /^re-check$/i });
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute('title', expect.stringMatching(/no stored email/i));
  });

  it('re-checks the entry and closes it once it leaves the queue', async () => {
    const spy = vi
      .spyOn(API.reconciliation, 'recheckUnassigned')
      .mockResolvedValue('queued_for_ai');
    const onClose = vi.fn();
    renderWithQuery(
      <UnassignedInspector record={{ ...baseRecord, can_recheck: true }} onClose={onClose} />
    );
    fireEvent.click(screen.getByRole('button', { name: /^re-check$/i }));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(spy).toHaveBeenCalledWith('u1');
    spy.mockRestore();
  });

  it('keeps the entry open when nothing new was found', async () => {
    const spy = vi
      .spyOn(API.reconciliation, 'recheckUnassigned')
      .mockResolvedValue('no_ai_available');
    const onClose = vi.fn();
    renderWithQuery(
      <UnassignedInspector record={{ ...baseRecord, can_recheck: true }} onClose={onClose} />
    );
    fireEvent.click(screen.getByRole('button', { name: /^re-check$/i }));
    await waitFor(() => expect(spy).toHaveBeenCalled());
    expect(onClose).not.toHaveBeenCalled();
    spy.mockRestore();
  });

  it('offers the re-check prompt inside the missing-instrument diagnosis only', () => {
    const missing = { ...baseRecord, can_recheck: true, reason: 'gate3_failed:missing_instrument' };
    const { unmount } = renderWithQuery(
      <UnassignedInspector record={missing} onClose={() => {}} />
    );
    expect(screen.getAllByRole('button', { name: /^re-check$/i })).toHaveLength(2);
    expect(screen.getByText(/learned a pattern for this bank since/i)).toBeInTheDocument();
    unmount();
    renderWithQuery(
      <UnassignedInspector record={{ ...baseRecord, can_recheck: true }} onClose={() => {}} />
    );
    expect(screen.getAllByRole('button', { name: /^re-check$/i })).toHaveLength(1);
  });
});
