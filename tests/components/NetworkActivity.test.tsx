import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import NetworkActivity from '@/components/NetworkActivity';
import { API } from '@/lib/ipc';

vi.mock('@/lib/ipc', () => ({ API: { network: { getActivityList: vi.fn() } } }));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;
const getActivityList = () => asMock(API.network.getActivityList);

const entry = (over = {}) => ({
  id: 'log1',
  timestamp: '2026-01-15T10:00:00Z',
  method: 'POST',
  domain: 'gmail.googleapis.com',
  url_redacted: 'https://gmail.googleapis.com/…/messages',
  bytes_sent: 512,
  bytes_received: 4096,
  status_code: 200,
  ...over,
});

beforeEach(() => {
  vi.clearAllMocks();
  getActivityList().mockResolvedValue([]);
});

describe('NetworkActivity', () => {
  it('always shows the outbound-channel disclosure', async () => {
    render(<NetworkActivity />);
    expect(screen.getByText(/Outbound Channels Disclosure/)).toBeTruthy();
  });

  it('loads the activity list on mount', async () => {
    render(<NetworkActivity />);
    await waitFor(() => expect(getActivityList()).toHaveBeenCalledTimes(1));
  });

  it('shows a loading state before the first response lands', () => {
    getActivityList().mockReturnValue(new Promise(() => {}));
    render(<NetworkActivity />);
    expect(screen.getByText(/Loading network activity/)).toBeTruthy();
  });

  it('reports an empty log rather than an empty table', async () => {
    render(<NetworkActivity />);
    expect(await screen.findByText(/No outbound requests recorded yet/)).toBeTruthy();
  });

  it('renders one row per recorded request', async () => {
    getActivityList().mockResolvedValue([entry(), entry({ id: 'log2', method: 'GET' })]);
    render(<NetworkActivity />);
    expect(await screen.findByText('POST')).toBeTruthy();
    expect(screen.getByText('GET')).toBeTruthy();
    expect(screen.getAllByText('gmail.googleapis.com')).toHaveLength(2);
  });

  it('shows only the redacted URL', async () => {
    getActivityList().mockResolvedValue([entry()]);
    render(<NetworkActivity />);
    expect(await screen.findByText('https://gmail.googleapis.com/…/messages')).toBeTruthy();
  });

  it.each(['bytes_sent', 'bytes_received', 'status_code'])(
    'renders a dash for a missing %s',
    async (field) => {
      getActivityList().mockResolvedValue([entry({ [field]: null })]);
      render(<NetworkActivity />);
      expect(await screen.findByText('-')).toBeTruthy();
    }
  );

  it('renders a zero byte count rather than a dash', async () => {
    getActivityList().mockResolvedValue([entry({ bytes_sent: 0 })]);
    render(<NetworkActivity />);
    expect(await screen.findByText('0')).toBeTruthy();
  });

  it('surfaces the failure message', async () => {
    getActivityList().mockRejectedValue(new Error('ipc unavailable'));
    render(<NetworkActivity />);
    expect(await screen.findByText('ipc unavailable')).toBeTruthy();
  });

  it('falls back to generic copy for a non-Error rejection', async () => {
    getActivityList().mockRejectedValue('kaboom');
    render(<NetworkActivity />);
    expect(await screen.findByText(/Failed to fetch network activity/)).toBeTruthy();
  });

  it('refetches when Refresh is pressed', async () => {
    render(<NetworkActivity />);
    await waitFor(() => expect(getActivityList()).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole('button', { name: /refresh/i }));
    await waitFor(() => expect(getActivityList()).toHaveBeenCalledTimes(2));
  });

  it('clears a previous error on a successful refresh', async () => {
    getActivityList().mockRejectedValueOnce(new Error('ipc unavailable'));
    render(<NetworkActivity />);
    expect(await screen.findByText('ipc unavailable')).toBeTruthy();
    getActivityList().mockResolvedValue([entry()]);
    fireEvent.click(screen.getByRole('button', { name: /refresh/i }));
    await waitFor(() => expect(screen.queryByText('ipc unavailable')).toBeNull());
  });

  it('disables Refresh while a fetch is in flight', async () => {
    getActivityList().mockReturnValue(new Promise(() => {}));
    render(<NetworkActivity />);
    expect(screen.getByRole('button', { name: /refresh/i })).toHaveProperty('disabled', true);
  });
});
