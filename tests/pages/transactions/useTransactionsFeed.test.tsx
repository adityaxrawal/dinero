// The infinite-scroll sentinel. Its IntersectionObserver callback and the
// disconnect on teardown are the only parts of this hook not reachable by
// rendering the feed, and a leaked observer keeps firing fetchNextPage
// against an unmounted feed.
import { useEffect } from 'react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render } from '@testing-library/react';
import { useTransactionsFeed } from '@/pages/transactions/useTransactionsFeed';
import { useTransactionsInfiniteList } from '@/hooks/queries/useTransactionsInfiniteList';
import { useTransactionSearch } from '@/hooks/queries/useTransactionSearch';

vi.mock('@/hooks/queries/useTransactionsInfiniteList', () => ({
  useTransactionsInfiniteList: vi.fn(),
}));
vi.mock('@/hooks/queries/useTransactionSearch', () => ({ useTransactionSearch: vi.fn() }));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;
const fetchNextPage = vi.fn();

let observerCallback: (entries: Array<{ isIntersecting: boolean }>) => void = () => {};
const observe = vi.fn();
const disconnect = vi.fn();

class FakeIntersectionObserver {
  constructor(cb: (entries: Array<{ isIntersecting: boolean }>) => void) {
    observerCallback = cb;
  }
  observe = observe;
  disconnect = disconnect;
  unobserve = vi.fn();
  takeRecords = vi.fn();
  root = null;
  rootMargin = '';
  thresholds = [];
}

let latest: ReturnType<typeof useTransactionsFeed>;

/** Mounts the hook with its sentinel attached to a real node, so the effect
 *  sees a non-null ref on its first run exactly as the feed does. */
function Harness({ isSearching, query = '' }: { isSearching: boolean; query?: string }) {
  const feed = useTransactionsFeed({}, query, isSearching);
  useEffect(() => {
    latest = feed;
  });
  // Handing the hook's own ref object to the node it is meant to observe is
  // the whole point of this harness; the rule cannot see that it is not a
  // read of `.current` during render.
  // eslint-disable-next-line react-hooks/refs
  return <div ref={feed.sentinelRef} />;
}

function setup({ hasNextPage = true, isFetchingNextPage = false, isSearching = false } = {}) {
  asMock(useTransactionsInfiniteList).mockReturnValue({
    data: { pages: [{ records: [{ id: 't1', transaction_date: '2026-05-01' }], total: 7 }] },
    hasNextPage,
    isFetchingNextPage,
    fetchNextPage,
    isLoading: false,
  });
  asMock(useTransactionSearch).mockReturnValue({ data: [], isLoading: false });
  return render(<Harness isSearching={isSearching} />);
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal('IntersectionObserver', FakeIntersectionObserver);
});

afterEach(() => vi.unstubAllGlobals());

describe('useTransactionsFeed infinite scroll', () => {
  it('fetches the next page when the sentinel scrolls into view', () => {
    setup();
    expect(observe).toHaveBeenCalled();

    observerCallback([{ isIntersecting: true }]);

    expect(fetchNextPage).toHaveBeenCalledTimes(1);
  });

  it('does nothing while the sentinel is off screen', () => {
    setup();

    observerCallback([{ isIntersecting: false }]);

    expect(fetchNextPage).not.toHaveBeenCalled();
  });

  it('does not stack a second fetch while one is already in flight', () => {
    setup({ isFetchingNextPage: true });

    observerCallback([{ isIntersecting: true }]);

    expect(fetchNextPage).not.toHaveBeenCalled();
  });

  it('never observes once the last page has loaded', () => {
    setup({ hasNextPage: false });

    expect(observe).not.toHaveBeenCalled();
  });

  it('never observes while a search is active', () => {
    // Search results are not paged, so the sentinel would fetch list pages
    // that are not being displayed.
    setup({ isSearching: true });

    expect(observe).not.toHaveBeenCalled();
  });

  it('disconnects the observer on unmount', () => {
    const { unmount } = setup();
    unmount();

    expect(disconnect).toHaveBeenCalled();
  });

  it('reports the list total when browsing and the result count when searching', () => {
    setup();
    expect(latest.total).toBe(7);

    asMock(useTransactionSearch).mockReturnValue({
      data: [{ id: 's1', transaction_date: '2026-05-01' }],
      isLoading: false,
    });
    render(<Harness isSearching query="coffee" />);
    expect(latest.total).toBe(1);
  });
});
