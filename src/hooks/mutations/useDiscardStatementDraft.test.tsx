import { describe, it, expect, vi } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useDiscardStatementDraft } from './useDiscardStatementDraft';
import { API } from '@/lib/ipc';

vi.mock('@/lib/ipc', () => ({
  API: { statements: { discardDraft: vi.fn() } },
}));

describe('useDiscardStatementDraft', () => {
  it('calls API.statements.discardDraft with the draft id', async () => {
    vi.mocked(API.statements.discardDraft).mockResolvedValue({ status: 'discarded' });
    const queryClient = new QueryClient();
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useDiscardStatementDraft(), { wrapper });

    result.current.mutate('draft_1');

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(API.statements.discardDraft).toHaveBeenCalledWith('draft_1');
  });
});
