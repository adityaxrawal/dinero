// The "Ready to review" section of the unprocessed-items queue. The
// actionable-groups half is already exercised via UnprocessedItemsQueue;
// this section and its per-item review hand-off are not.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { ReviewableSection } from '@/components/statements/queue/QueueSections';
import { useGlobalState } from '@/lib/GlobalStateContext';

const openReviewModal = vi.fn();
vi.mock('@/lib/GlobalStateContext', () => ({ useGlobalState: vi.fn() }));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;

const item = (over = {}) => ({
  draft_id: 'draft_1',
  issuer_name: 'HDFC Bank',
  masked_identifier: '4321',
  ...over,
});

type Items = Parameters<typeof ReviewableSection>[0]['items'];
const renderSection = (items: unknown[]) =>
  render(<ReviewableSection items={items as Items} />);

beforeEach(() => {
  vi.clearAllMocks();
  asMock(useGlobalState).mockReturnValue({ openReviewModal });
});

describe('ReviewableSection', () => {
  it('labels the section and counts what is waiting', () => {
    renderSection([item(), item({ draft_id: 'draft_2' })]);

    expect(screen.getByRole('region', { name: 'Ready to review' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { level: 3 })).toHaveTextContent('Ready to review (2)');
    expect(screen.getAllByRole('button', { name: /^Review/ })).toHaveLength(2);
  });

  it('identifies each draft by issuer and masked account', () => {
    renderSection([item()]);

    expect(screen.getByText('HDFC Bank •••4321')).toBeInTheDocument();
  });

  it('falls back to placeholder digits when the account is unknown', () => {
    renderSection([item({ masked_identifier: null })]);

    expect(screen.getByText('HDFC Bank •••????')).toBeInTheDocument();
  });

  it('falls back to generic copy when the issuer could not be read', () => {
    renderSection([item({ issuer_name: null })]);

    expect(screen.getByText('Statement ready for review')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Review statement' })).toBeInTheDocument();
  });

  it('opens the review modal for the draft that was clicked', () => {
    renderSection([item(), item({ draft_id: 'draft_2', issuer_name: 'ICICI' })]);

    fireEvent.click(screen.getByRole('button', { name: 'Review ICICI' }));

    expect(openReviewModal).toHaveBeenCalledTimes(1);
    expect(openReviewModal).toHaveBeenCalledWith('draft_2');
  });

  it('renders an empty section rather than disappearing', () => {
    // The heading stays so the queue layout does not jump as items drain.
    renderSection([]);

    expect(screen.getByRole('region', { name: 'Ready to review' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Review/ })).not.toBeInTheDocument();
  });
});
