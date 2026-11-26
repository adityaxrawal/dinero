/**
 * Processing history section of the statements screen.
 */
import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { FileSearch } from 'lucide-react';
import { API } from '@/lib/ipc';
import { useToast } from '@/hooks/use-toast';
import { getErrorMessage } from '@/lib/errorMapping';
import { useStatementsList } from '@/hooks/queries/useStatementsList';
import SectionHeading from '@/components/settings/SectionHeading';
import StatementPdfViewerModal from '@/components/statements/StatementPdfViewerModal';
import HistoryRow from './HistoryRow';

type Statement = NonNullable<ReturnType<typeof useStatementsList>['data']>[number];

/** The list of processed statements. */
function HistoryList({
  loading,
  onRetry,
  history,
  onViewPdf,
  onDeletePdf,
}: {
  loading: boolean;
  /** Set when the history failed to load; shown instead of an empty list. */
  onRetry?: (() => void) | undefined;
  history: Statement[];
  onViewPdf: (id: string) => void;
  onDeletePdf: (stmt: Statement) => void;
}) {
  const navigate = useNavigate();

  if (loading) {
    return <div className="p-8 text-center text-[13px] text-emerald-ink/70">Loading history…</div>;
  }
  // A failed load must not read as "No statements uploaded yet" (audit F8-8).
  if (onRetry) {
    return (
      <div className="p-8 text-center text-[13px] text-emerald-ink/70" role="alert">
        Couldn't load your statement history.{' '}
        <button type="button" onClick={onRetry} className="underline font-medium text-emerald-ink">
          Try again
        </button>
      </div>
    );
  }
  if (history.length === 0) {
    return (
      <div className="p-8 text-center text-[13px] text-emerald-ink/70">
        No statements uploaded yet.
      </div>
    );
  }

  return (
    <div className="divide-y divide-emerald-ink/5">
      {history.map((stmt) => (
        <HistoryRow
          key={stmt.id}
          stmt={stmt}
          onViewPdf={() => onViewPdf(stmt.id)}
          onDeletePdf={() => onDeletePdf(stmt)}
          onViewTransactions={() =>
            navigate(
              stmt.instrument_id
                ? `/transactions?instrument=${stmt.instrument_id}`
                : `/transactions?search=${encodeURIComponent(stmt.file_name)}`
            )
          }
        />
      ))}
    </div>
  );
}

/** Processing history section of the statements screen. */
export default function ProcessingHistorySection({ refresh }: { refresh: () => void }) {
  const { toast } = useToast();
  const { data: history = [], isLoading, isError, refetch } = useStatementsList();
  const [viewingPdfStatementId, setViewingPdfStatementId] = useState<string | null>(null);

  /** Deletes a stored statement PDF after confirmation. */
  const handleDeletePdf = async (stmt: Statement) => {
    try {
      await API.statements.deletePdf(stmt.id);
      toast({ title: 'PDF deleted', description: 'The stored PDF has been removed.' });
      refresh();
    } catch (e) {
      toast({
        title: 'Could not delete PDF',
        description: getErrorMessage(e, 'Please try again.'),
        variant: 'destructive',
      });
    }
  };

  return (
    <section aria-label="Processing History" className="animate-in fade-in duration-300">
      <SectionHeading
        icon={FileSearch}
        title="Processing History"
        description="Previously parsed and extracted statements."
      />

      <div className="bg-champagne/50 rounded-xl overflow-hidden border border-emerald-ink/10 flex flex-col">
        <HistoryList
          loading={isLoading}
          onRetry={isError ? () => refetch() : undefined}
          history={history}
          onViewPdf={setViewingPdfStatementId}
          onDeletePdf={handleDeletePdf}
        />
      </div>

      <StatementPdfViewerModal
        statementId={viewingPdfStatementId}
        open={viewingPdfStatementId !== null}
        onOpenChange={(open) => {
          if (!open) setViewingPdfStatementId(null);
        }}
      />
    </section>
  );
}
