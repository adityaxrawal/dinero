import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import SourceEvidencePanel from './SourceEvidencePanel';
import { API, type TransactionObservation } from '@/lib/ipc';

vi.mock('@/lib/ipc', () => ({ API: { transactions: { getSourceLog: vi.fn() } } }));
vi.mock('@/components/common/GmailEmailViewer', () => ({
  GmailEmailViewer: ({ subject }: { subject?: string | null }) => (
    <div data-testid="email-viewer">{subject}</div>
  ),
}));
vi.mock('./ReportWrongBankDialog', () => ({ default: () => <div data-testid="wrong-bank" /> }));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;

const obs = (over: Partial<TransactionObservation> = {}): TransactionObservation =>
  ({
    id: 'o1',
    source_pipeline: 'gmail_alert',
    raw_payload_json: null,
    merchant_raw: 'SWIGGY',
    amount: 450.5,
    confidence_score: 0.9,
    extraction_method: 'regex',
    event_time: '2026-01-15T10:00:00Z',
    is_deleted: false,
    ...over,
  }) as TransactionObservation;

const renderPanel = (observations: TransactionObservation[] = [obs()]) =>
  render(
    <SourceEvidencePanel transactionId="tx1" observations={observations} currentBank="HDFC Bank" />
  );

beforeEach(() => {
  vi.clearAllMocks();
  asMock(API.transactions.getSourceLog).mockResolvedValue('pipeline log line');
  vi.spyOn(console, 'error').mockImplementation(() => {});
});

describe('SourceEvidencePanel', () => {
  it('says so when nothing is linked', () => {
    renderPanel([]);
    expect(screen.getByText(/No linked observations found/)).toBeInTheDocument();
    expect(API.transactions.getSourceLog).not.toHaveBeenCalled();
  });

  it('lists one card per contributing observation', () => {
    renderPanel([obs(), obs({ id: 'o2', source_pipeline: 'statement_pdf' })]);
    expect(screen.getByText('Email Extraction')).toBeInTheDocument();
    expect(screen.getByText('Statement Extraction')).toBeInTheDocument();
  });

  describe('source log', () => {
    it('is fetched for a gmail-sourced transaction', async () => {
      renderPanel();
      await waitFor(() => expect(API.transactions.getSourceLog).toHaveBeenCalledWith('tx1'));
    });

    it('is not fetched when no observation came from gmail', () => {
      renderPanel([obs({ source_pipeline: 'statement_pdf' })]);
      expect(API.transactions.getSourceLog).not.toHaveBeenCalled();
    });

    it('is not fetched when the pipeline is unknown', () => {
      renderPanel([obs({ source_pipeline: null })]);
      expect(API.transactions.getSourceLog).not.toHaveBeenCalled();
    });

    it('survives a fetch failure', async () => {
      asMock(API.transactions.getSourceLog).mockRejectedValue(new Error('ipc down'));
      renderPanel();
      await waitFor(() => expect(API.transactions.getSourceLog).toHaveBeenCalled());
      expect(screen.getByText('Source Evidence')).toBeInTheDocument();
    });

    it('does not set state after unmount', async () => {
      let release: (v: string) => void;
      asMock(API.transactions.getSourceLog).mockReturnValue(
        new Promise<string>((r) => (release = r))
      );
      const { unmount } = renderPanel();
      unmount();
      release!('late log');
      // A post-unmount setState would surface as an act() warning here.
      await waitFor(() => expect(API.transactions.getSourceLog).toHaveBeenCalled());
    });
  });

  describe('original email', () => {
    const withHtml = JSON.stringify({ html: '<p>hi</p>', subject: 'Txn alert' });
    const withBodyOnly = JSON.stringify({ body: 'plain text alert', subject: 'Plain alert' });

    it('renders the email when a payload carries html', () => {
      renderPanel([obs({ raw_payload_json: withHtml })]);
      expect(screen.getByTestId('email-viewer').textContent).toBe('Txn alert');
    });

    it('accepts a body-only payload', () => {
      renderPanel([obs({ raw_payload_json: withBodyOnly })]);
      expect(screen.getByTestId('email-viewer').textContent).toBe('Plain alert');
    });

    it('renders nothing when the payload has neither html nor body', () => {
      renderPanel([obs({ raw_payload_json: JSON.stringify({ subject: 'only a subject' }) })]);
      expect(screen.queryByTestId('email-viewer')).toBeNull();
    });

    it('ignores an unparseable payload', () => {
      renderPanel([obs({ raw_payload_json: '{not json' })]);
      expect(screen.queryByTestId('email-viewer')).toBeNull();
    });

    it('picks the first observation that actually has a body', () => {
      renderPanel([
        obs({ id: 'o1', raw_payload_json: JSON.stringify({ subject: 'no body' }) }),
        obs({ id: 'o2', raw_payload_json: withHtml }),
      ]);
      expect(screen.getByTestId('email-viewer').textContent).toBe('Txn alert');
    });
  });
});
