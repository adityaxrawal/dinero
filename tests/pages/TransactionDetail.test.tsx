// Covers the TransactionDetail page shell itself -- the guards and
// conditional sections that its children's tests (transactionDetail/
// detailPanels.test.tsx) can't reach because they import the children
// directly and never render the page.
//
// The guard ordering below is the specific thing worth pinning: isLoading
// must be checked before the detail/tx guard, or the spinner is unreachable
// and the page renders blank for the whole load.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import TransactionDetail from '@/pages/TransactionDetail';

const navigate = vi.fn();
const openRawSource = vi.fn();
let params: { id?: string } = { id: 'tx1' };
let form: Record<string, unknown> = {};

vi.mock('react-router-dom', () => ({
  useParams: () => params,
  useNavigate: () => navigate,
}));
vi.mock('@/lib/formatCustomDate', () => ({ formatCustomDate: (d: string) => `on ${d}` }));
vi.mock('@/components/transactions/useTransactionForm', () => ({
  useTransactionForm: () => form,
}));
vi.mock('@/pages/transactionDetail/useRawSource', () => ({
  useRawSource: () => ({
    isOpen: false,
    setIsOpen: vi.fn(),
    isLoading: false,
    data: null,
    open: openRawSource,
  }),
}));

// Children are stubbed: each already has its own spec, and stubbing keeps
// this file testing the shell's wiring rather than their internals.
vi.mock('@/pages/transactionDetail/TransactionHero', () => ({
  default: () => <div data-testid="hero" />,
}));
vi.mock('@/pages/transactionDetail/MetadataCard', () => ({
  default: ({ originalName, onViewSource }: { originalName: string; onViewSource: () => void }) => (
    <button type="button" data-testid="metadata" onClick={onViewSource}>
      {originalName || 'no-name'}
    </button>
  ),
}));
vi.mock('@/pages/transactionDetail/InstrumentCard', () => ({
  default: () => <div data-testid="instrument" />,
}));
vi.mock('@/pages/transactionDetail/AuditCard', () => ({ default: () => <div data-testid="audit" /> }));
vi.mock('@/pages/transactionDetail/RawSourceDialog', () => ({ default: () => null }));
vi.mock('@/components/transactions/SourceEvidencePanel', () => ({
  default: ({
    transactionId,
    currentBank,
  }: {
    transactionId: string;
    currentBank: string | null;
  }) => <div data-testid="evidence">{`${transactionId}:${currentBank ?? 'no-bank'}`}</div>,
}));
vi.mock('@/components/transactions/EmiInstallmentTimeline', () => ({
  default: ({ emiGroupId }: { emiGroupId: string }) => <div data-testid="emi">{emiGroupId}</div>,
}));

const tx = (over: Record<string, unknown> = {}) => ({
  id: 'tx1',
  merchant_display_name: 'Swiggy',
  emi_group_id: null,
  created_at: null,
  updated_at: null,
  ...over,
});

const loadedForm = (over: Record<string, unknown> = {}) => ({
  isLoading: false,
  detail: { observations: [] },
  tx: tx(),
  category: null,
  isDebit: true,
  amountStr: '245.43',
  setAmountStr: vi.fn(),
  setDirection: vi.fn(),
  instrument: { issuer_name: 'HDFC' },
  ...over,
});

beforeEach(() => {
  vi.clearAllMocks();
  params = { id: 'tx1' };
  form = loadedForm();
});

describe('TransactionDetail shell guards', () => {
  it('renders nothing without a route id', () => {
    params = {};
    const { container } = render(<TransactionDetail />);
    expect(container).toBeEmptyDOMElement();
  });

  it('shows the spinner while loading, even though detail and tx are still undefined', () => {
    // Regression lock: guarding on !detail/!tx first made this unreachable.
    form = loadedForm({ isLoading: true, detail: undefined, tx: undefined });
    render(<TransactionDetail />);
    expect(screen.getByRole('status', { name: 'Loading transaction' })).toBeInTheDocument();
    expect(screen.queryByTestId('hero')).not.toBeInTheDocument();
  });

  it('renders nothing when the query settled without a transaction', () => {
    form = loadedForm({ detail: undefined, tx: undefined });
    const { container } = render(<TransactionDetail />);
    expect(container).toBeEmptyDOMElement();
  });

  it('renders every always-on section once loaded', () => {
    render(<TransactionDetail />);
    expect(screen.getByTestId('hero')).toBeInTheDocument();
    expect(screen.getByTestId('metadata')).toBeInTheDocument();
    expect(screen.getByTestId('instrument')).toBeInTheDocument();
    expect(screen.getByTestId('audit')).toBeInTheDocument();
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });
});

describe('TransactionDetail conditional sections', () => {
  it('shows the EMI timeline only when the transaction belongs to an EMI group', () => {
    const { unmount } = render(<TransactionDetail />);
    expect(screen.queryByTestId('emi')).not.toBeInTheDocument();
    unmount();

    form = loadedForm({ tx: tx({ emi_group_id: 'emi9' }) });
    render(<TransactionDetail />);
    expect(screen.getByTestId('emi')).toHaveTextContent('emi9');
  });

  it('omits the timestamp footer when neither timestamp is set', () => {
    render(<TransactionDetail />);
    expect(screen.queryByText(/Recorded/)).not.toBeInTheDocument();
  });

  it('shows only "Recorded" when the row has never been updated', () => {
    form = loadedForm({ tx: tx({ created_at: 'T1', updated_at: 'T1' }) });
    render(<TransactionDetail />);
    expect(screen.getByText(/Recorded on T1/)).toBeInTheDocument();
    expect(screen.queryByText(/Updated/)).not.toBeInTheDocument();
  });

  it('appends "Updated" once the row diverges from its creation time', () => {
    form = loadedForm({ tx: tx({ created_at: 'T1', updated_at: 'T2' }) });
    render(<TransactionDetail />);
    expect(screen.getByText(/Recorded on T1/)).toBeInTheDocument();
    expect(screen.getByText(/Updated on T2/)).toBeInTheDocument();
  });

  it('falls back to empty strings and nulls for absent merchant and instrument', () => {
    form = loadedForm({ tx: tx({ merchant_display_name: null }), instrument: undefined });
    render(<TransactionDetail />);
    expect(screen.getByTestId('metadata')).toHaveTextContent('no-name');
    expect(screen.getByTestId('evidence')).toHaveTextContent('tx1:no-bank');
  });
});

describe('TransactionDetail actions', () => {
  it('navigates back to the list', () => {
    render(<TransactionDetail />);
    fireEvent.click(screen.getByRole('button', { name: 'Back to transactions' }));
    expect(navigate).toHaveBeenCalledWith('/transactions');
  });

  it('opens the raw-source dialog from the metadata card', () => {
    render(<TransactionDetail />);
    fireEvent.click(screen.getByTestId('metadata'));
    expect(openRawSource).toHaveBeenCalled();
  });
});
