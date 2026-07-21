import { describe, it, expect, vi } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useCommitStatementDraft } from './useCommitStatementDraft';
import { API } from '@/lib/ipc';

vi.mock('@/lib/ipc', () => ({
  API: { statements: { commitDraft: vi.fn() } },
}));

describe('useCommitStatementDraft', () => {
  it('calls API.statements.commitDraft with the draft id, metadata, and rows', async () => {
    (API.statements.commitDraft as any).mockResolvedValue({ status: 'committed', statement_id: 'stmt_1' });
    const queryClient = new QueryClient();
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useCommitStatementDraft(), { wrapper });

    const metadata = {
      issuerName: 'HDFC', maskedIdentifier: '1111', instrumentType: 'credit_card',
      billingPeriodStart: null, billingPeriodEnd: null, dueDate: null, statementDate: null,
      currentBalance: null, minimumDue: null,
    };
    result.current.mutate({ draftId: 'draft_1', metadata, rows: [] });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(API.statements.commitDraft).toHaveBeenCalledWith('draft_1', metadata, []);
  });
});
