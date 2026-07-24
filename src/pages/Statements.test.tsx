import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import Statements from './Statements';
import { useStatementsList } from '@/hooks/queries/useStatementsList';

vi.mock('@/hooks/queries/useStatementsList');

vi.mock('@/lib/GlobalStateContext', () => ({
  useGlobalState: () => ({
    openPasswordModal: vi.fn(),
    instrumentModalOpen: false,
    pendingInstrumentStatementId: null,
    pendingInstrumentFilename: '',
    pendingInstrumentIssuerHint: '',
    pendingInstrumentReason: '',
    closeInstrumentModal: vi.fn(),
  }),
}));

vi.mock('@/components/statements/StatementUploadDropzone', () => ({ default: () => null }));
vi.mock('@/components/statements/UnprocessedItemsQueue', () => ({ default: () => null }));
vi.mock('@/components/statements/PasswordPromptModal', () => ({ default: () => null }));
vi.mock('@/components/statements/StatementReviewModal', () => ({ default: () => null }));
vi.mock('@/components/statements/StatementPdfViewerModal', () => ({ default: () => null }));

function renderStatements() {
  const queryClient = new QueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={['/statements?section=history']}>
        <Statements />
      </MemoryRouter>
    </QueryClientProvider>
  );
}

describe('Statements — Processing History', () => {
  it('shows View PDF and Delete PDF for a completed statement with a retained PDF', () => {
    (useStatementsList as any).mockReturnValue({
      isLoading: false,
      data: [
        {
          id: 'stmt_1',
          date: '2026-01-01T00:00:00Z',
          file_name: 'statement.pdf',
          // Real backend value (src-tauri/migrations/20260101000025_statements_source_type_and_checks.sql):
          // parse_status CHECK(parse_status IN ('queued','processing','parsed','failed')).
          // 'PROCESSED' is never actually written by the backend.
          status: 'parsed',
          instrument_id: 'inst_1',
          issuer_name: 'HDFC Bank',
          masked_identifier: '3825',
          instrument_type: 'credit_card',
          pdf_available: true,
        },
      ],
    });

    renderStatements();

    expect(screen.getByRole('button', { name: /view statement pdf/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /delete stored pdf/i })).toBeInTheDocument();
  });
});
