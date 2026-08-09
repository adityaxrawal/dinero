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

interface UnassignedInspectorProps {
  record: UnassignedTransactionRecord | undefined;
  onClose: () => void;
  inline?: boolean;
}

function InspectorHeader({ title, inline, onClose }: { title: string; inline: boolean; onClose: () => void }) {
  return (
    <div
      className={cn('flex items-start justify-between p-5 flex-shrink-0', inline && 'pt-0')}
      style={{ borderBottom: '1px solid rgba(6,78,59,0.1)' }}
    >
      <div className="min-w-0 flex-1 pr-3">
        <span className="text-[9px] font-bold px-1.5 py-0.5 rounded-sm mb-2 inline-block uppercase tracking-wider bg-red-500/10 text-red-700">
          Action Required
        </span>
        <p className="text-[15px] font-semibold text-[#064E3B]">{title}</p>
      </div>

      <button
        type="button"
        className="w-8 h-8 flex items-center justify-center rounded-lg transition-colors hover:bg-[#064E3B]/10 text-[#064E3B]/60 hover:text-[#064E3B] flex-shrink-0"
        onClick={onClose}
        aria-label="Close inspector"
      >
        <X className="w-5 h-5" />
      </button>
    </div>
  );
}

export default function UnassignedInspector({
  record,
  onClose,
  inline = false,
}: UnassignedInspectorProps) {
  const isOpen = !!record;
  const { data: instruments = [] } = useInstrumentsList();
  const form = useUnassignedForm(record, onClose);

  const asideClasses = inspectorPanelClasses(inline, isOpen);
  const asideStyle = inspectorPanelStyle(inline, isOpen);

  if (!record) {
    return (
      <aside className={asideClasses} role="complementary" aria-hidden={true} style={asideStyle} />
    );
  }

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
          <FailureDiagnosis record={record} />

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
          canSubmit={form.canSubmit}
          isPending={form.isPending}
          onDismiss={form.handleDismiss}
          onSave={form.handleSave}
        />
      </div>
    </aside>
  );
}
