import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import Reconciliation from '@/pages/Reconciliation';
import type { ClusterRecord, ClusterMember, UnassignedTransactionRecord } from '@/lib/ipc';

let clusters: ClusterRecord[] = [];
let unassigned: UnassignedTransactionRecord[] = [];
let clustersLoading = false;
let unassignedLoading = false;
let section = 'clusters';
const setSearchParams = vi.fn((next: { section: string }) => {
  section = next.section;
});

vi.mock('react-router-dom', () => ({
  useSearchParams: () => [new URLSearchParams({ section }), setSearchParams],
}));
vi.mock('@/hooks/queries/useReconciliationClusters', () => ({
  useReconciliationClusters: () => ({ data: clusters, isLoading: clustersLoading }),
}));
let llmPending = 0;
let unassignedError = false;
const recheckAllMutate = vi.fn();
vi.mock('@/hooks/mutations/useRecheckUnassigned', () => ({
  useRecheckAllUnassigned: () => ({ mutate: recheckAllMutate, isPending: false }),
  recheckAllMessage: () => ({ title: 'Re-check finished', description: '' }),
}));
vi.mock('@/hooks/use-toast', () => ({ useToast: () => ({ toast: vi.fn() }) }));
let llmState: 'ready' | 'no_model' | 'ineligible' = 'ready';
vi.mock('@/hooks/queries/useLlmQueueStatus', () => ({
  useLlmQueueStatus: () => ({ data: { pending: llmPending, state: llmState } }),
}));
vi.mock('@/hooks/queries/useUnassignedTransactions', () => ({
  useUnassignedTransactions: () => ({
    data: unassigned,
    isLoading: unassignedLoading,
    isError: unassignedError,
    refetch: vi.fn(),
  }),
}));
vi.mock('@/components/reconciliation/ReconciliationInspector', () => ({
  default: ({ cluster }: { cluster?: ClusterRecord }) => (
    <div data-testid="cluster-inspector">{cluster?.id ?? 'none'}</div>
  ),
}));
vi.mock('@/components/reconciliation/UnassignedInspector', () => ({
  default: ({ record }: { record?: UnassignedTransactionRecord }) => (
    <div data-testid="unassigned-inspector">{record?.id ?? 'none'}</div>
  ),
}));

const member = (over: Partial<ClusterMember> = {}): ClusterMember =>
  ({
    id: 'm1',
    member_role: 'incoming',
    observation_id: 'obs1',
    canonical_transaction_id: null,
    merchant: 'Swiggy',
    amount_minor: 45050,
    currency: 'INR',
    direction: 'debit',
    date: '2026-01-01',
    match_score: null,
    ...over,
  }) as ClusterMember;

const cluster = (over: Partial<ClusterRecord> = {}): ClusterRecord => ({
  id: 'c1',
  reason: 'amount_date',
  members_count: 2,
  members: [member()],
  created_at: new Date().toISOString(),
  explanation: 'looks similar',
  ...over,
});

const item = (over: Partial<UnassignedTransactionRecord> = {}): UnassignedTransactionRecord => ({
  id: 'u1',
  observation_id: 'obs9',
  reason: 'extraction_failed',
  status: 'pending',
  created_at: '2026-01-15T00:00:00Z',
  merchant_raw: 'HDFC Bank',
  amount_minor: 45050,
  currency: 'INR',
  direction: 'debit',
  event_time: '2026-01-15T10:00:00Z',
  source_message_id: 'msg1',
  body_snippet: 'Your card was debited',
  raw_payload_json: null,
  ...over,
});

beforeEach(() => {
  vi.clearAllMocks();
  clusters = [cluster()];
  unassigned = [item()];
  clustersLoading = false;
  unassignedLoading = false;
  unassignedError = false;
  section = 'clusters';
  llmPending = 0;
  llmState = 'ready';
});

describe('Reconciliation queue', () => {
  it('shows a loading state', () => {
    clustersLoading = true;
    render(<Reconciliation />);
    expect(screen.getByText('Review Inbox')).toBeInTheDocument();
    expect(screen.queryByText('Swiggy')).toBeNull();
  });

  it('counts each section in its tab', () => {
    clusters = [cluster(), cluster({ id: 'c2' })];
    render(<Reconciliation />);
    expect(screen.getByRole('tab', { name: /Possible duplicates/ }).textContent).toContain('2');
    expect(screen.getByRole('tab', { name: /Needs details/ }).textContent).toContain('1');
  });

  it('switches section through the URL', () => {
    render(<Reconciliation />);
    fireEvent.click(screen.getByRole('tab', { name: /Needs details/ }));
    expect(setSearchParams).toHaveBeenCalledWith({ section: 'unassigned' });
  });

  it('reports an empty cluster queue', () => {
    clusters = [];
    render(<Reconciliation />);
    expect(screen.getByText('No pending clusters.')).toBeInTheDocument();
  });

  it('reports an empty unassigned queue', () => {
    section = 'unassigned';
    unassigned = [];
    render(<Reconciliation />);
    expect(screen.getByText('No unassigned transactions.')).toBeInTheDocument();
  });
});

describe('cluster list item', () => {
  it('shows the incoming merchant and entry count', () => {
    render(<Reconciliation />);
    expect(screen.getByText('Swiggy')).toBeInTheDocument();
    expect(screen.getByText('2 entries')).toBeInTheDocument();
  });

  it('renders a debit amount as negative', () => {
    render(<Reconciliation />);
    expect(screen.getByText('−₹450.50')).toBeInTheDocument();
  });

  it('renders a credit amount as positive', () => {
    clusters = [cluster({ members: [member({ direction: 'credit' })] })];
    render(<Reconciliation />);
    expect(screen.getByText('+₹450.50')).toBeInTheDocument();
  });

  it('falls back when the cluster has no incoming member', () => {
    clusters = [cluster({ members: [member({ member_role: 'candidate_a' })] })];
    render(<Reconciliation />);
    expect(screen.getByText('Match requires review')).toBeInTheDocument();
  });

  it('flags a cluster left pending for more than three days', () => {
    const old = new Date(Date.now() - 10 * 864e5).toISOString();
    clusters = [cluster({ created_at: old })];
    render(<Reconciliation />);
    expect(screen.getByTitle('Pending more than 3 days')).toBeInTheDocument();
  });

  it('does not flag a fresh cluster as stale', () => {
    render(<Reconciliation />);
    expect(screen.queryByTitle('Pending more than 3 days')).toBeNull();
  });

  it('omits the age when the cluster has no created_at', () => {
    clusters = [cluster({ created_at: null })];
    render(<Reconciliation />);
    expect(screen.getByText('Swiggy')).toBeInTheDocument();
  });
});

describe('unassigned list item', () => {
  beforeEach(() => {
    section = 'unassigned';
  });

  it('shows the resolved bank name and its avatar letter', () => {
    render(<Reconciliation />);
    expect(screen.getByText('HDFC Bank')).toBeInTheDocument();
    expect(screen.getByText('H')).toBeInTheDocument();
  });

  it('marks the selected row with aria-current (a11y)', () => {
    render(<Reconciliation />);
    const row = screen.getByText('HDFC Bank').closest('button')!;
    expect(row).not.toHaveAttribute('aria-current');
    fireEvent.click(row);
    expect(screen.getByText('HDFC Bank').closest('button')).toHaveAttribute('aria-current', 'true');
  });

  it('shows the formatted amount', () => {
    render(<Reconciliation />);
    expect(screen.getByText('−₹450.50')).toBeInTheDocument();
  });

  it.each([
    ['extraction_failed', 'Missing details'],
    ['issuer_name_not_found', 'Unknown card or bank'],
    ['something_else', 'Needs review'],
  ])('labels reason %s as %s', (reason, label) => {
    unassigned = [item({ reason })];
    render(<Reconciliation />);
    expect(screen.getByText(label)).toBeInTheDocument();
  });
});

describe('inspector pane', () => {
  it('prompts to pick a cluster before one is selected', () => {
    render(<Reconciliation />);
    expect(screen.getByText('Select a cluster to resolve')).toBeInTheDocument();
  });

  it('prompts generically in the unassigned section', () => {
    section = 'unassigned';
    render(<Reconciliation />);
    expect(screen.getByText('Select an item to view details')).toBeInTheDocument();
  });

  it('opens the cluster inspector on selection', () => {
    render(<Reconciliation />);
    fireEvent.click(screen.getByText('Swiggy'));
    expect(screen.getByTestId('cluster-inspector').textContent).toBe('c1');
  });

  it('toggles the cluster inspector closed on a second click', () => {
    render(<Reconciliation />);
    fireEvent.click(screen.getByText('Swiggy'));
    fireEvent.click(screen.getByText('Swiggy'));
    expect(screen.queryByTestId('cluster-inspector')).toBeNull();
  });

  it('opens the unassigned inspector on selection', () => {
    section = 'unassigned';
    render(<Reconciliation />);
    fireEvent.click(screen.getByText('HDFC Bank'));
    expect(screen.getByTestId('unassigned-inspector').textContent).toBe('u1');
  });

  it('shows how many emails the AI is still reading, even when the queue is empty', () => {
    clusters = [];
    unassigned = [];
    llmPending = 3;
    render(<Reconciliation />);
    expect(screen.getByRole('status').textContent).toContain('AI is reading 3 emails');
    expect(screen.getByText('All Caught Up!')).toBeTruthy();
  });

  it('tells the user to download a model instead of claiming the AI is reading', () => {
    llmPending = 50;
    llmState = 'no_model';
    render(<Reconciliation />);
    const line = screen.getByRole('status').textContent ?? '';
    expect(line).toContain('50 emails are waiting for the local AI');
    expect(line).toContain('Download a model');
    expect(line).not.toContain('AI is reading');
  });

  it('says when this Mac cannot run the AI', () => {
    llmPending = 1;
    llmState = 'ineligible';
    render(<Reconciliation />);
    expect(screen.getByRole('status').textContent).toContain("doesn't have enough memory");
  });

  it('shows an error with retry instead of "All Caught Up" when the queue fails to load', () => {
    unassignedError = true;
    render(<Reconciliation />);
    expect(screen.queryByText('All Caught Up!')).toBeNull();
    expect(screen.getByRole('alert').textContent).toContain("Couldn't load the review queue");
  });

  // Held emails are listed and editable while the AI works (owner decision Q-2).
  it('labels an entry the AI has not read yet', () => {
    section = 'unassigned';
    unassigned = [item({ waiting_for_ai: true })];
    render(<Reconciliation />);
    expect(screen.getByText('Waiting for AI')).toBeTruthy();
  });

  // With no tab in the URL, open the tab that has work (audit F9-13).
  it('opens the Needs details tab when only review entries exist', () => {
    section = '';
    clusters = [];
    render(<Reconciliation />);
    expect(screen.getByRole('tab', { name: /Needs details/ }).getAttribute('aria-selected')).toBe('true');
  });

  it('shows no AI line when nothing is pending', () => {
    render(<Reconciliation />);
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('offers Re-check all on the unassigned tab when an entry keeps its email', () => {
    section = 'unassigned';
    unassigned = [item({ can_recheck: true })];
    render(<Reconciliation />);
    fireEvent.click(screen.getByText('Re-check all'));
    expect(recheckAllMutate).toHaveBeenCalledTimes(1);
  });

  it('hides Re-check all when no entry can be re-checked, or on the clusters tab', () => {
    section = 'unassigned';
    unassigned = [item({ can_recheck: false })];
    const { unmount } = render(<Reconciliation />);
    expect(screen.queryByText('Re-check all')).toBeNull();
    unmount();
    section = 'clusters';
    unassigned = [item({ can_recheck: true })];
    render(<Reconciliation />);
    expect(screen.queryByText('Re-check all')).toBeNull();
  });
});
