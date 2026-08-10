// Covers the InstrumentDetail page shell -- guards, the saved-passwords
// gate, and the forget-password toasts. instrumentDetail/instrumentCards
// .test.tsx tests the cards directly and never mounts this page.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import InstrumentDetail from '@/pages/InstrumentDetail';

const navigate = vi.fn();
const toast = vi.fn();
const mutate = vi.fn();
let params: { id?: string } = { id: 'i1' };
let form: Record<string, unknown> = {};

vi.mock('react-router-dom', () => ({
  useParams: () => params,
  useNavigate: () => navigate,
}));
vi.mock('@/hooks/use-toast', () => ({ useToast: () => ({ toast }) }));
vi.mock('@/lib/errorMapping', () => ({
  getErrorToast: (e: unknown) => ({ title: 'Mapped', description: String(e) }),
}));
vi.mock('@/components/instruments/useInstrumentForm', () => ({
  useInstrumentForm: () => form,
}));

vi.mock('@/pages/instrumentDetail/EditableDetailsCard', () => ({
  default: () => <div data-testid="details" />,
}));
vi.mock('@/pages/instrumentDetail/SavedPasswordsCard', () => ({
  default: ({ onForget }: { onForget: (id: string) => void }) => (
    <button type="button" data-testid="passwords" onClick={() => onForget('pw1')}>
      forget
    </button>
  ),
}));
vi.mock('@/pages/instrumentDetail/RecentTransactionsCard', () => ({
  default: ({ onViewAll }: { onViewAll: () => void }) => (
    <button type="button" data-testid="recent" onClick={onViewAll}>
      view all
    </button>
  ),
}));
vi.mock('@/pages/instrumentDetail/StatementHistoryCard', () => ({
  default: () => <div data-testid="statements" />,
}));

const loadedForm = (over: Record<string, unknown> = {}) => ({
  isLoading: false,
  inst: {
    id: 'i1',
    issuer_name: 'HDFC',
    instrument_type: 'credit_card',
    masked_identifier: '1234',
  },
  instrumentPasswords: [],
  instrumentStatements: [],
  forgetPassword: { mutate, isPending: false },
  ...over,
});

beforeEach(() => {
  vi.clearAllMocks();
  params = { id: 'i1' };
  form = loadedForm();
});

describe('InstrumentDetail guards', () => {
  it('renders nothing without a route id', () => {
    params = {};
    const { container } = render(<InstrumentDetail />);
    expect(container).toBeEmptyDOMElement();
  });

  it('shows the spinner while the instrument loads', () => {
    form = loadedForm({ isLoading: true, inst: undefined });
    render(<InstrumentDetail />);
    expect(screen.getByRole('status', { name: 'Loading instrument' })).toBeInTheDocument();
  });

  it('keeps the spinner when the query settled without an instrument', () => {
    // `!inst` shares the loading branch on purpose -- a missing instrument
    // must not fall through to a header reading its fields.
    form = loadedForm({ inst: undefined });
    render(<InstrumentDetail />);
    expect(screen.getByRole('status', { name: 'Loading instrument' })).toBeInTheDocument();
    expect(screen.queryByTestId('details')).not.toBeInTheDocument();
  });

  it('renders the header and always-on cards once loaded', () => {
    render(<InstrumentDetail />);
    expect(screen.getByRole('heading', { name: 'HDFC' })).toBeInTheDocument();
    expect(screen.getByText('1234')).toBeInTheDocument();
    expect(screen.getByText('Credit Card')).toBeInTheDocument();
    expect(screen.getByTestId('details')).toBeInTheDocument();
    expect(screen.getByTestId('recent')).toBeInTheDocument();
    expect(screen.getByTestId('statements')).toBeInTheDocument();
  });
});

describe('InstrumentDetail saved passwords', () => {
  it('hides the card when no passwords are stored', () => {
    render(<InstrumentDetail />);
    expect(screen.queryByTestId('passwords')).not.toBeInTheDocument();
  });

  it('shows the card once a password exists', () => {
    form = loadedForm({ instrumentPasswords: [{ id: 'pw1' }] });
    render(<InstrumentDetail />);
    expect(screen.getByTestId('passwords')).toBeInTheDocument();
  });

  it('confirms a forgotten password', () => {
    mutate.mockImplementation((_id, opts) => opts.onSuccess());
    form = loadedForm({ instrumentPasswords: [{ id: 'pw1' }] });
    render(<InstrumentDetail />);
    fireEvent.click(screen.getByTestId('passwords'));
    expect(mutate).toHaveBeenCalledWith('pw1', expect.anything());
    expect(toast).toHaveBeenCalledWith({ title: 'Saved password forgotten' });
  });

  it('surfaces a mapped error toast when forgetting fails', () => {
    mutate.mockImplementation((_id, opts) => opts.onError(new Error('locked')));
    form = loadedForm({ instrumentPasswords: [{ id: 'pw1' }] });
    render(<InstrumentDetail />);
    fireEvent.click(screen.getByTestId('passwords'));
    expect(toast).toHaveBeenCalledWith(
      expect.objectContaining({ variant: 'destructive', title: 'Mapped' })
    );
  });
});

describe('InstrumentDetail navigation', () => {
  it('navigates back to the instruments list', () => {
    render(<InstrumentDetail />);
    fireEvent.click(screen.getByRole('button', { name: 'Back to instruments' }));
    expect(navigate).toHaveBeenCalledWith('/instruments');
  });

  it('deep-links to this instrument when viewing all transactions', () => {
    render(<InstrumentDetail />);
    fireEvent.click(screen.getByTestId('recent'));
    expect(navigate).toHaveBeenCalledWith('/transactions?instrument=i1');
  });
});
