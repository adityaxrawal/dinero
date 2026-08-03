import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import TransactionInspector from './TransactionInspector';
import type { CategoryRecord } from '@/lib/ipc';

const navigate = vi.fn();
const handleSave = vi.fn();
const handleDelete = vi.fn();
const resetForm = vi.fn();
let form: Record<string, unknown>;

vi.mock('react-router-dom', () => ({ useNavigate: () => navigate }));
vi.mock('./useTransactionForm', () => ({ useTransactionForm: () => form }));
vi.mock('./SourceEvidencePanel', () => ({
  default: () => <div data-testid="evidence-panel" />,
}));
vi.mock('./EmiInstallmentTimeline', () => ({
  default: ({ emiGroupId }: { emiGroupId: string }) => (
    <div data-testid="emi-timeline">{emiGroupId}</div>
  ),
}));

const tx = (over = {}) => ({
  id: 'tx1',
  merchant_display_name: 'Swiggy',
  amount: 450.5,
  amount_minor: 45050,
  direction: 'debit',
  currency: 'INR',
  status: 'posted',
  emi_group_id: null,
  best_event_time: '2026-01-15T10:00:00Z',
  notes: '',
  category_id: 'cat1',
  instrument_id: 'inst1',
  original_amount_minor: null,
  original_currency: null,
  exchange_rate: null,
  balance_after_transaction: null,
  reference_id: 'REF123',
  location: null,
  ...over,
});

const baseForm = (over: Record<string, unknown> = {}) => ({
  detail: { transaction: tx(), observations: [] },
  isLoading: false,
  tags: [],
  availableTags: [],
  merchant: 'Swiggy',
  setMerchant: vi.fn(),
  categoryId: 'cat1',
  setCategoryId: vi.fn(),
  notes: '',
  setNotes: vi.fn(),
  amountStr: '450.5',
  setAmountStr: vi.fn(),
  direction: 'debit',
  setDirection: vi.fn(),
  eventTime: '2026-01-15T10:00:00Z',
  setEventTime: vi.fn(),
  instrumentId: 'inst1',
  setInstrumentId: vi.fn(),
  instruments: [{ id: 'inst1', issuer_name: 'HDFC Bank', masked_identifier: '8841' }],
  newTag: '',
  setNewTag: vi.fn(),
  showSavedConfirm: false,
  isDirty: false,
  resetForm,
  updateFields: { isPending: false },
  softDelete: { isPending: false },
  tx: tx(),
  amount: 450.5,
  hasEmi: false,
  isDebit: true,
  instrument: { id: 'inst1', issuer_name: 'HDFC Bank' },
  category: { name: 'Food', color: '#ff0000' },
  isForeignCurrency: false,
  handleSave,
  handleAddTag: vi.fn(),
  handleRemoveTag: vi.fn(),
  handleDelete,
  ...over,
});

const category: CategoryRecord = {
  id: 'cat1',
  parent_id: null,
  name: 'Food',
  source_type: 'system',
  mcc_code: null,
  monthly_budget_minor: null,
  is_deleted: false,
  created_at: null,
  color: '#ff0000',
  icon: null,
};

const renderInspector = (props = {}) =>
  render(
    <TransactionInspector
      transactionId="tx1"
      onClose={vi.fn()}
      categories={[category]}
      {...props}
    />
  );

beforeEach(() => {
  vi.clearAllMocks();
  form = baseForm();
});

describe('TransactionInspector', () => {
  it('renders nothing when no transaction is selected', () => {
    const { container } = renderInspector({ transactionId: null });
    expect(container).toBeEmptyDOMElement();
  });

  it('renders the panel for a selected transaction', () => {
    renderInspector();
    expect(screen.getByRole('complementary', { name: 'Transaction detail' })).toBeTruthy();
  });

  describe('header', () => {
    it('navigates to the full page view', () => {
      renderInspector();
      fireEvent.click(screen.getByLabelText('Open full page'));
      expect(navigate).toHaveBeenCalledWith('/transactions/tx1');
    });

    it('closes the panel', () => {
      const onClose = vi.fn();
      renderInspector({ onClose });
      fireEvent.click(screen.getByLabelText('Close inspector'));
      expect(onClose).toHaveBeenCalled();
    });
  });

  describe('hero stat', () => {
    it('shows the amount once loaded', () => {
      renderInspector();
      expect(screen.getByDisplayValue('450.5')).toBeTruthy();
    });

    it('is hidden while loading', () => {
      form = baseForm({ isLoading: true, tx: undefined });
      renderInspector();
      expect(screen.queryByDisplayValue('450.5')).toBeNull();
    });
  });

  describe('tabs', () => {
    it('opens on the details tab', () => {
      renderInspector();
      expect(screen.getByRole('tab', { name: /details/i }).getAttribute('aria-selected')).toBe(
        'true'
      );
    });

    it('counts the observations on the evidence tab', () => {
      form = baseForm({
        detail: { transaction: tx(), observations: [{ id: 'o1' }, { id: 'o2' }] },
      });
      renderInspector();
      expect(screen.getByRole('tab', { name: /evidence/i }).textContent).toContain('2');
    });

    it('switches to the evidence panel', () => {
      renderInspector();
      fireEvent.click(screen.getByRole('tab', { name: /evidence/i }));
      expect(screen.getByTestId('evidence-panel')).toBeTruthy();
    });

    it('disables the EMI tab for a non-EMI transaction', () => {
      renderInspector();
      expect(screen.getByRole('tab', { name: /emi/i })).toHaveProperty('disabled', true);
    });

    it('shows the EMI timeline for an EMI transaction', () => {
      form = baseForm({ hasEmi: true, tx: tx({ emi_group_id: 'emi1' }) });
      renderInspector();
      fireEvent.click(screen.getByRole('tab', { name: /emi/i }));
      expect(screen.getByTestId('emi-timeline').textContent).toBe('emi1');
    });

    it('returns to the details tab when a different transaction is selected', () => {
      const { rerender } = renderInspector();
      fireEvent.click(screen.getByRole('tab', { name: /evidence/i }));
      rerender(
        <TransactionInspector transactionId="tx2" onClose={vi.fn()} categories={[]} />
      );
      expect(screen.getByRole('tab', { name: /details/i }).getAttribute('aria-selected')).toBe(
        'true'
      );
    });
  });

  describe('footer', () => {
    it('disables save when there is nothing to save', () => {
      renderInspector();
      expect(screen.getByRole('button', { name: /save changes/i })).toHaveProperty(
        'disabled',
        true
      );
    });

    it('enables save once the form is dirty', () => {
      form = baseForm({ isDirty: true });
      renderInspector();
      const save = screen.getByRole('button', { name: /save changes/i });
      expect(save).toHaveProperty('disabled', false);
      fireEvent.click(save);
      expect(handleSave).toHaveBeenCalled();
    });

    it('offers a reset only while dirty', () => {
      renderInspector();
      expect(screen.queryByRole('button', { name: /reset/i })).toBeNull();
      form = baseForm({ isDirty: true });
      renderInspector();
      fireEvent.click(screen.getAllByRole('button', { name: /reset/i })[0]);
      expect(resetForm).toHaveBeenCalled();
    });

    it('swaps the unsaved-edits banner for a confirmation once saved', () => {
      form = baseForm({ isDirty: true });
      const { unmount } = renderInspector();
      expect(screen.getByText('Unsaved edits')).toBeTruthy();
      unmount();

      form = baseForm({ isDirty: true, showSavedConfirm: true });
      renderInspector();
      expect(screen.queryByText('Unsaved edits')).toBeNull();
      expect(screen.getByRole('status').textContent).toContain('Changes saved successfully');
    });

    it('deletes the transaction', () => {
      renderInspector();
      fireEvent.click(screen.getByTitle('Delete Transaction'));
      expect(handleDelete).toHaveBeenCalled();
    });

    it('disables save and shows progress while a save is in flight', () => {
      form = baseForm({ isDirty: true, updateFields: { isPending: true } });
      renderInspector();
      const save = screen.getByRole('button', { name: /saving edits/i });
      expect(save).toHaveProperty('disabled', true);
    });

    it('disables delete while a delete is in flight', () => {
      form = baseForm({ softDelete: { isPending: true } });
      renderInspector();
      expect(screen.getByTitle('Delete Transaction')).toHaveProperty('disabled', true);
    });
  });

  describe('Cmd/Ctrl+S shortcut', () => {
    it('saves when the form is dirty', () => {
      form = baseForm({ isDirty: true });
      renderInspector();
      fireEvent.keyDown(window, { key: 's', metaKey: true });
      expect(handleSave).toHaveBeenCalled();
    });

    it('works with Ctrl as well as Cmd', () => {
      form = baseForm({ isDirty: true });
      renderInspector();
      fireEvent.keyDown(window, { key: 's', ctrlKey: true });
      expect(handleSave).toHaveBeenCalled();
    });

    it('does nothing when there are no changes', () => {
      renderInspector();
      fireEvent.keyDown(window, { key: 's', metaKey: true });
      expect(handleSave).not.toHaveBeenCalled();
    });

    it('ignores a bare "s" keypress', () => {
      form = baseForm({ isDirty: true });
      renderInspector();
      fireEvent.keyDown(window, { key: 's' });
      expect(handleSave).not.toHaveBeenCalled();
    });

    it('stops listening once unmounted', () => {
      form = baseForm({ isDirty: true });
      const { unmount } = renderInspector();
      unmount();
      fireEvent.keyDown(window, { key: 's', metaKey: true });
      expect(handleSave).not.toHaveBeenCalled();
    });
  });
});
