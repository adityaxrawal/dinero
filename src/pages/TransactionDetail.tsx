import { useParams, useNavigate } from 'react-router-dom';
import { ArrowLeft, Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { formatCustomDate } from '@/lib/formatCustomDate';
import { useTransactionForm } from '@/components/transactions/useTransactionForm';
import SourceEvidencePanel from '@/components/transactions/SourceEvidencePanel';
import EmiInstallmentTimeline from '@/components/transactions/EmiInstallmentTimeline';
import { useRawSource } from './transactionDetail/useRawSource';
import TransactionHero from './transactionDetail/TransactionHero';
import MetadataCard from './transactionDetail/MetadataCard';
import InstrumentCard from './transactionDetail/InstrumentCard';
import AuditCard from './transactionDetail/AuditCard';
import RawSourceDialog from './transactionDetail/RawSourceDialog';

/**
 * TASK-FE-010 (Doc 30): full editable field display (category, merchant
 * display name, tags, notes), SourceEvidencePanel, EmiInstallmentTimeline
 * (only when emi_group_id present). Replaces the TASK-FE-009 placeholder.
 */
export default function TransactionDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();

  const form = useTransactionForm(id, () => navigate('/transactions'));
  const rawSource = useRawSource(id);

  const { detail, tx } = form;
  if (!id) return null;

  // isLoading must be checked before the detail/tx guard: both are undefined
  // while the query is in flight, so guarding first made this spinner
  // unreachable and rendered a blank page for the whole load.
  if (form.isLoading) {
    return (
      <div
        className="flex-1 flex items-center justify-center h-40"
        role="status"
        aria-label="Loading transaction"
      >
        <Loader2 className="w-5 h-5 animate-spin text-muted-foreground" aria-hidden="true" />
      </div>
    );
  }

  if (!detail || !tx) return null;

  return (
    // AppLayout's <main> is overflow-hidden, so every routed page owns its
    // own scroll container -- this page's content can exceed the viewport
    // (Correction section + Save/Delete buttons below Provenance), and
    // without this wrapper that overflow was silently clipped with no way
    // to reach it at all. mx-auto centers the fixed max-w-3xl column
    // instead of it pinning to the left edge on a wide window.
    <div className="flex-1 h-full overflow-y-auto">
      <div className="space-y-6 animate-in fade-in duration-300 max-w-3xl mx-auto p-6 lg:p-10">
        <Button
          variant="ghost"
          size="sm"
          onClick={() => navigate('/transactions')}
          aria-label="Back to transactions"
        >
          <ArrowLeft className="w-4 h-4 mr-1" aria-hidden="true" /> Back
        </Button>

        <TransactionHero
          tx={tx}
          category={form.category}
          isDebit={form.isDebit}
          amountStr={form.amountStr}
          setAmountStr={form.setAmountStr}
          setDirection={form.setDirection}
        />

        <MetadataCard
          form={form}
          originalName={tx.merchant_display_name ?? ''}
          onViewSource={rawSource.open}
        />

        <InstrumentCard form={form} tx={tx} />

        <AuditCard tx={tx} />

        <SourceEvidencePanel
          transactionId={id}
          observations={detail.observations}
          currentBank={form.instrument?.issuer_name ?? null}
        />

        {tx.emi_group_id && <EmiInstallmentTimeline emiGroupId={tx.emi_group_id} />}

        {(tx.created_at || tx.updated_at) && (
          <p className="text-xs text-muted-foreground text-center font-mono opacity-80">
            {tx.created_at && `Recorded ${formatCustomDate(tx.created_at)}`}
            {tx.updated_at &&
              tx.updated_at !== tx.created_at &&
              ` · Updated ${formatCustomDate(tx.updated_at)}`}
          </p>
        )}

        <RawSourceDialog
          open={rawSource.isOpen}
          onOpenChange={rawSource.setIsOpen}
          isLoading={rawSource.isLoading}
          data={rawSource.data}
        />
      </div>
    </div>
  );
}
