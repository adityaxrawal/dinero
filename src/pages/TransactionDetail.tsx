import { useParams, useNavigate } from 'react-router-dom';
import { ArrowLeft, Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useTransactionDetail } from '@/hooks/queries/useTransactionDetail';

/**
 * TASK-FE-009 placeholder: TransactionRow now navigates here on click
 * instead of opening the old inline drawer. Intentionally minimal — the
 * real build-out (editable fields, SourceEvidencePanel, EmiInstallmentTimeline)
 * is TASK-FE-010, done immediately after this one in the same session so the
 * route is never left broken for more than one commit.
 */
export default function TransactionDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { data: tx, isLoading } = useTransactionDetail(id);

  return (
    <div className="space-y-6 animate-in fade-in duration-300">
      <Button variant="ghost" size="sm" onClick={() => navigate('/transactions')} aria-label="Back to transactions">
        <ArrowLeft className="w-4 h-4 mr-1" aria-hidden="true" /> Back
      </Button>
      {isLoading ? (
        <div className="flex items-center justify-center h-40" role="status">
          <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" aria-hidden="true" />
        </div>
      ) : (
        <pre className="text-xs bg-secondary/50 rounded-md p-4 overflow-auto">{JSON.stringify(tx, null, 2)}</pre>
      )}
    </div>
  );
}
