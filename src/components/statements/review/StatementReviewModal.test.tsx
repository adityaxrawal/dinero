// The staged-draft review modal: the last chance to correct extraction before
// rows are committed, so its two phases and its edits are pinned here.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import StatementReviewModal from '../StatementReviewModal';
import { API } from '@/lib/ipc';
import { useGlobalState } from '@/lib/GlobalStateContext';

vi.mock('@/lib/GlobalStateContext', () => ({ useGlobalState: vi.fn() }));
vi.mock('@/hooks/use-toast', () => ({ useToast: () => ({ toast: vi.fn() }) }));
vi.mock('@/lib/ipc', () => ({
  API: { statements: { getDraftPdf: vi.fn(), getDraft: vi.fn() } },
}));

const commit = vi.fn();
const discard = vi.fn();
vi.mock('@/hooks/mutations/useCommitStatementDraft', () => ({
  useCommitStatementDraft: () => ({ mutate: commit, isPending: false }),
}));
vi.mock('@/hooks/mutations/useDiscardStatementDraft', () => ({
  useDiscardStatementDraft: () => ({ mutate: discard, isPending: false }),
}));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;
const closeReviewModal = vi.fn();

const DRAFT = {
  issuer_name: 'HDFC',
  masked_identifier: '4321',
  instrument_type: 'credit_card',
  billing_period_start: '2026-06-01',
  billing_period_end: '2026-06-30',
  due_date: '2026-07-15',
  statement_date: '2026-07-01',
  current_balance: 250050,
  minimum_due: 120000,
  rows: [
    {
      transaction_date: '2026-06-05',
      merchant_raw: 'Swiggy',
      amount_minor: 24550,
      currency: 'INR',
      direction: 'debit',
      reference_id: null,
      row_index: 0,
      llm_extracted: false,
    },
  ],
};

function mockState(over: Record<string, unknown> = {}) {
  asMock(useGlobalState).mockImplementation(() => ({
    reviewModalOpen: true,
    activeDraftId: 'd1',
    processingProgress: null,
    closeReviewModal,
    ...over,
  }));
}

beforeEach(() => {
  vi.clearAllMocks();
  mockState();
  vi.stubGlobal('atob', (s: string) => s);
  URL.createObjectURL = vi.fn(() => 'blob:pdf');
  URL.revokeObjectURL = vi.fn();
  asMock(API.statements.getDraftPdf).mockResolvedValue('pdfbytes');
  asMock(API.statements.getDraft).mockResolvedValue(DRAFT);
});

describe('StatementReviewModal while extracting', () => {
  it('shows the stage label and blocks submission', async () => {
    mockState({ processingProgress: { stage: 'extracting_rows', percent: 60 } });
    asMock(API.statements.getDraft).mockResolvedValue(undefined);
    render(<StatementReviewModal />);

    expect(await screen.findByText('Extracting transactions…')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Submit' })).toBeDisabled();
  });

  it('falls back to the parsing label for an unknown stage', async () => {
    mockState({ processingProgress: { stage: 'nonsense', percent: 5 } });
    asMock(API.statements.getDraft).mockResolvedValue(undefined);
    render(<StatementReviewModal />);
    expect(await screen.findByText('Extracting data from your statement…')).toBeInTheDocument();
  });
});

describe('StatementReviewModal once staged', () => {
  it('prefills metadata, converting money out of minor units', async () => {
    render(<StatementReviewModal />);
    expect(await screen.findByLabelText('Bank name')).toHaveValue('HDFC');
    expect(screen.getByLabelText('Card number (last 4)')).toHaveValue('4321');
    expect(screen.getByLabelText('Current balance (₹)')).toHaveValue(2500.5);
    expect(screen.getByLabelText('Minimum due (₹)')).toHaveValue(1200);
  });

  it('lists the extracted rows in major units', async () => {
    render(<StatementReviewModal />);
    expect(await screen.findByLabelText('Row 1 merchant')).toHaveValue('Swiggy');
    expect(screen.getByLabelText('Row 1 amount')).toHaveValue(245.5);
    expect(screen.getByText('Transactions (1)')).toBeInTheDocument();
  });

  it('adds and removes rows', async () => {
    render(<StatementReviewModal />);
    fireEvent.click(await screen.findByRole('button', { name: /Add row/ }));
    expect(screen.getByText('Transactions (2)')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Delete row 2' }));
    expect(screen.getByText('Transactions (1)')).toBeInTheDocument();
  });

  it('converts an edited amount back into minor units on submit', async () => {
    render(<StatementReviewModal />);
    fireEvent.change(await screen.findByLabelText('Row 1 amount'), { target: { value: '99.99' } });
    fireEvent.click(screen.getByRole('button', { name: 'Submit' }));

    await waitFor(() => expect(commit).toHaveBeenCalled());
    expect(commit.mock.calls[0][0].rows[0].amount_minor).toBe(9999);
  });

  it('commits the edited metadata under the active draft id', async () => {
    render(<StatementReviewModal />);
    fireEvent.change(await screen.findByLabelText('Bank name'), { target: { value: 'Axis' } });
    fireEvent.click(screen.getByRole('button', { name: 'Submit' }));

    await waitFor(() => expect(commit).toHaveBeenCalled());
    expect(commit.mock.calls[0][0].draftId).toBe('d1');
    expect(commit.mock.calls[0][0].metadata.issuerName).toBe('Axis');
  });

  it('discards the draft when cancelled rather than leaving it staged', async () => {
    render(<StatementReviewModal />);
    fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }));
    expect(discard).toHaveBeenCalledWith('d1', expect.anything());
    expect(closeReviewModal).toHaveBeenCalled();
  });

  it('says the PDF is loading until the bytes arrive', async () => {
    asMock(API.statements.getDraftPdf).mockRejectedValue(new Error('gone'));
    render(<StatementReviewModal />);
    expect(await screen.findByText('Loading PDF…')).toBeInTheDocument();
  });
});
