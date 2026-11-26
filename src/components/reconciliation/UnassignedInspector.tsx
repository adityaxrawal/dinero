/**
 * Inspector for a transaction that could not be attributed to an instrument.
 *
 * Pairs a diagnosis of why attribution failed with the form to resolve it, so the
 * user understands the cause rather than only being asked to fix the symptom.
 */
import { OUTCOME } from '@/lib/outcomeVocabulary';
import { X } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { UnassignedTransactionRecord } from '@/lib/ipc';
import { useInstrumentsList } from '@/hooks/queries/useInstrumentsList';
import { inspectorPanelClasses, inspectorPanelStyle } from '@/components/layout/inspectorPanel';
import { reasonTitle, parseEmailEvidence } from './unassigned/diagnostics';
import { useUnassignedForm } from './unassigned/useUnassignedForm';
import FailureDiagnosis from './unassigned/FailureDiagnosis';
import ExtractedDataForm from './unassigned/ExtractedDataForm';
import SourceEmailEvidence from './unassigned/SourceEmailEvidence';
import UnassignedActions from './unassigned/UnassignedActions';
import { useToast } from '@/hooks/use-toast';
import { getErrorToast } from '@/lib/errorMapping';
import { recheckMessage, useRecheckUnassigned } from '@/hooks/mutations/useRecheckUnassigned';

interface UnassignedInspectorProps {
  record: UnassignedTransactionRecord | undefined;
  onClose: () => void;
  /** The entry left the queue in the background (e.g. the AI booked it) while
   * open: the last copy stays on screen with the user's edits (audit F9-10). */
  resolvedElsewhere?: boolean;
  inline?: boolean;
}

/** Header for the unassigned inspector. */
function InspectorHeader({
  title,
  inline,
  onClose,
}: {
  title: string;
  inline: boolean;
  onClose: () => void;
}) {
  return (
    <div
      className={cn('flex items-start justify-between p-5 flex-shrink-0', inline && 'pt-0')}
      style={{ borderBottom: '1px solid rgba(6,78,59,0.1)' }}
    >
      <div className="min-w-0 flex-1 pr-3">
        <span className="text-[11px] font-bold px-1.5 py-0.5 rounded-sm mb-2 inline-block uppercase tracking-wider bg-red-500/10 text-red-700">
          {OUTCOME.needsReview}
        </span>
        <p className="text-[15px] font-semibold text-emerald-ink">{title}</p>
      </div>

      <button
        type="button"
        className="w-8 h-8 flex items-center justify-center rounded-lg transition-colors hover:bg-emerald-ink/10 text-emerald-ink/60 hover:text-emerald-ink flex-shrink-0"
        onClick={onClose}
        aria-label="Close inspector"
      >
        <X className="w-5 h-5" />
      </button>
    </div>
  );
}

/**
 * Inspector for a transaction that could not be attributed.
 *
 * Pairs a diagnosis of why attribution failed with the form to resolve it, so
 * the user understands the cause rather than only the symptom.
 */
export default function UnassignedInspector({
  record,
  onClose,
  inline = false,
  resolvedElsewhere = false,
}: UnassignedInspectorProps) {
  const isOpen = !!record;
  const { data: instruments = [] } = useInstrumentsList();
  const form = useUnassignedForm(record, onClose);
  const recheck = useRecheckUnassigned();
  const { toast } = useToast();

  const asideClasses = inspectorPanelClasses(inline, isOpen);
  const asideStyle = inspectorPanelStyle(inline, isOpen);

  if (!record) {
    return (
      <aside className={asideClasses} role="complementary" aria-hidden={true} style={asideStyle} />
    );
  }

  const recordId = record.id;
  /** Re-checks this entry; it leaves the queue unless nothing changed. */
  const handleRecheck = () =>
    recheck.mutate(recordId, {
      onSuccess: (outcome) => {
        toast(recheckMessage(outcome));
        if (outcome !== 'no_ai_available') onClose();
      },
      onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }),
    });
  const busy = form.isPending || recheck.isPending;

  const evidence = parseEmailEvidence(record);
  const hasEvidence = Boolean(evidence.html || evidence.text || record.body_snippet);

  return (
    <aside
      className={asideClasses}
      role="complementary"
      aria-label="Unassigned detail"
      aria-hidden={!isOpen}
      style={asideStyle}
    >
      <InspectorHeader title={reasonTitle(record.reason)} inline={inline} onClose={onClose} />

      <div
        className={cn(
          'flex-1 overflow-y-auto flex flex-col',
          inline ? 'py-6 px-4 md:px-8 max-w-5xl mx-auto w-full' : 'p-4'
        )}
      >
        <div className="flex-1 space-y-6">
          {resolvedElsewhere && (
            <p
              className="rounded-lg border border-amber-300 bg-amber-50 p-3 text-[12px] text-amber-900"
              role="alert"
            >
              This entry was already handled in the background (the local AI booked it) while you
              were editing. Your edits are still shown below but can't be saved here; check the
              ledger and edit the transaction there if needed.
            </p>
          )}
          {!resolvedElsewhere && form.updatedInBackground && (
            <p
              className="rounded-lg border border-amber-300 bg-amber-50 p-3 text-[12px] text-amber-900"
              role="status"
            >
              This entry was updated in the background. Your edits are kept.{' '}
              <button type="button" className="underline font-medium" onClick={form.loadLatest}>
                Load the new values
              </button>
            </p>
          )}
          {!resolvedElsewhere && record.waiting_for_ai && (
            <p
              className="rounded-lg border border-emerald-ink/15 bg-emerald-ink/5 p-3 text-[12px] text-emerald-ink"
              role="status"
            >
              The local AI hasn't read this email yet. You can fill it in now; if you save it, the
              AI's later answer is ignored.
            </p>
          )}
          <FailureDiagnosis
            record={record}
            onRecheck={handleRecheck}
            isRechecking={recheck.isPending}
          />

          <ExtractedDataForm
            form={form}
            instruments={instruments}
            sourceMessageId={record.source_message_id}
          />

          {hasEvidence && (
            <SourceEmailEvidence
              record={record}
              evidence={evidence}
              onQuickFill={form.applyQuickFill}
            />
          )}
        </div>

        <UnassignedActions
          canSubmit={form.canSubmit && !resolvedElsewhere}
          isPending={busy}
          canRecheck={record.can_recheck === true && !resolvedElsewhere}
          onRecheck={handleRecheck}
          onDismiss={form.handleDismiss}
          onSave={form.handleSave}
        />
      </div>
    </aside>
  );
}
