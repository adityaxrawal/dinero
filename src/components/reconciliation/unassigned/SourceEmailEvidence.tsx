/**
 * Shows the source email behind an unassigned transaction, as evidence.
 */
import { FileText } from 'lucide-react';
import type { UnassignedTransactionRecord } from '@/lib/ipc';
import { GmailEmailViewer } from '@/components/common/GmailEmailViewer';
import type { EmailEvidence } from './diagnostics';

/** Shows the source email as evidence for the resolution. */
export default function SourceEmailEvidence({
  record,
  evidence,
  onQuickFill,
}: {
  record: UnassignedTransactionRecord;
  evidence: EmailEvidence;
  onQuickFill: (fill: { field: string; value: string }) => void;
}) {
  return (
    <div>
      <h3 className="text-sm font-semibold mb-3 text-[#064E3B] flex items-center gap-2">
        <FileText className="w-4 h-4" /> Source Email Evidence
      </h3>
      <GmailEmailViewer
        html={evidence.html || undefined}
        text={evidence.text || record.body_snippet || undefined}
        sender={evidence.sender || record.merchant_raw || 'Bank Alert'}
        subject={evidence.subject}
        date={record.event_time}
        maxHeight="420px"
        onQuickFill={onQuickFill}
      />
    </div>
  );
}
