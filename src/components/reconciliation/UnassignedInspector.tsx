import { X, AlertTriangle, FileText, Trash2 } from 'lucide-react';
import { useQueryClient } from '@tanstack/react-query';
import { cn } from '@/lib/utils';
import type { UnassignedTransactionRecord } from '@/lib/ipc';
import { Button } from '@/components/ui/button';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

interface UnassignedInspectorProps {
  record: UnassignedTransactionRecord | undefined;
  onClose: () => void;
  inline?: boolean;
}

export default function UnassignedInspector({
  record,
  onClose,
  inline = false,
}: UnassignedInspectorProps) {
  const queryClient = useQueryClient();
  const isOpen = !!record;

  const asideClasses = cn(
    !inline && 'inspector-panel',
    !inline && (!record || !isOpen) && 'closed',
    inline && 'w-full h-full flex flex-col',
    !inline && 'flex-shrink-0'
  );

  const asideStyle = inline ? { backgroundColor: '#F8E7C9' } : { width: (record && isOpen) ? 'var(--inspector-width)' : 0 };

  const handleDismiss = async () => {
    if (!record) return;
    try {
      await API.reconciliation.dismissUnassigned(record.id);
      queryClient.invalidateQueries({ queryKey: queryKeys.reconciliation.unassigned() });
      onClose();
    } catch (err) {
      console.error('Failed to dismiss', err);
    }
  };

  if (!record) {
    return (
      <aside
        className={asideClasses}
        role="complementary"
        aria-hidden={true}
        style={asideStyle}
      />
    );
  }

  // Format currency
  const amountFormatted = typeof record.amount_minor === 'number' && record.currency
    ? new Intl.NumberFormat('en-IN', { style: 'currency', currency: record.currency }).format(record.amount_minor / 100)
    : 'Unknown';

  const reasonMap: Record<string, string> = {
    'extraction_failed': 'Failed to Extract Details',
    'issuer_name_not_found': 'Unknown Instrument',
  };

  const title = reasonMap[record.reason] || record.reason || 'Unresolved Issue';

  let emailHtml = '';
  let emailText = '';
  if (record.raw_payload_json) {
    try {
      const parsed = JSON.parse(record.raw_payload_json);
      emailHtml = parsed.html || '';
      emailText = parsed.body || parsed.snippet || '';
    } catch (e) {
      emailText = record.body_snippet || '';
    }
  } else if (record.body_snippet) {
    emailText = record.body_snippet;
  }

  return (
    <aside
      className={asideClasses}
      role="complementary"
      aria-label="Unassigned detail"
      aria-hidden={!isOpen}
      style={asideStyle}
    >
        {/* Header */}
        <div
          className={cn("flex items-start justify-between p-5 flex-shrink-0", inline && "pt-0")}
          style={{ borderBottom: '1px solid rgba(6,78,59,0.1)' }}
        >
          <div className="min-w-0 flex-1 pr-3">
            <span
              className="text-[9px] font-bold px-1.5 py-0.5 rounded-sm mb-2 inline-block uppercase tracking-wider bg-red-500/10 text-red-700"
            >
              Action Required
            </span>
            <p className="text-[15px] font-semibold text-[#064E3B]">
              {title}
            </p>
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

        {/* Content */}
        <div className={cn("flex-1 overflow-y-auto flex flex-col", inline ? "p-8 max-w-3xl mx-auto w-full" : "p-4")}>
          
          <div className="flex-1 space-y-6">
            
            {/* Why it failed */}
            <div className="bg-white/50 rounded-xl p-5 border border-red-200 shadow-sm">
              <h3 className="text-sm font-semibold mb-2 text-red-800 flex items-center gap-2">
                <AlertTriangle className="w-4 h-4" /> Why did this fail?
              </h3>
              <p className="text-[13px] text-[#064E3B]/80 leading-relaxed mb-4">
                {record.reason === 'extraction_failed' 
                  ? "Dinero's parser could not reliably identify the required transaction details (like amount, merchant, or date) from this message. This often happens with new or non-standard bank alert formats."
                  : record.reason === 'issuer_name_not_found'
                  ? "The transaction details were extracted successfully, but Dinero could not match the bank or card (instrument) mentioned in the message to any of your configured instruments."
                  : "An unexpected error occurred while processing this message."}
              </p>
              
              <div className="bg-red-50/50 rounded-lg p-3 text-xs text-red-900/80 font-medium">
                <strong>Tip:</strong> If you see this frequently for a specific bank, try switching to our Advanced LLM parser in Settings for better accuracy, or wait for an upcoming rules engine update.
              </div>
            </div>

            {/* Extracted Data */}
            <div>
              <h3 className="text-sm font-semibold mb-3 text-[#064E3B] flex items-center gap-2">
                <FileText className="w-4 h-4" /> Extracted Data
              </h3>
              <div className="bg-white rounded-xl border border-[#064E3B]/10 overflow-hidden text-[13px]">
                <div className="grid grid-cols-[120px_1fr] border-b border-[#064E3B]/5">
                  <div className="px-4 py-3 bg-[#064E3B]/5 font-medium text-[#064E3B]/70">Amount</div>
                  <div className="px-4 py-3 text-[#064E3B] font-mono">{amountFormatted}</div>
                </div>
                <div className="grid grid-cols-[120px_1fr] border-b border-[#064E3B]/5">
                  <div className="px-4 py-3 bg-[#064E3B]/5 font-medium text-[#064E3B]/70">Merchant</div>
                  <div className="px-4 py-3 text-[#064E3B]">{record.merchant_raw || <span className="text-[#064E3B]/40 italic">Not found</span>}</div>
                </div>
                {record.source_message_id && (
                  <div className="grid grid-cols-[120px_1fr] border-b border-[#064E3B]/5">
                    <div className="px-4 py-3 bg-[#064E3B]/5 font-medium text-[#064E3B]/70">Source ID</div>
                    <div className="px-4 py-3 text-[#064E3B]/70 font-mono text-[11px] truncate" title={record.source_message_id}>
                      {record.source_message_id}
                    </div>
                  </div>
                )}
                {(emailHtml || emailText) && (
                  <div className="grid grid-cols-[120px_1fr]">
                    <div className="px-4 py-3 bg-[#064E3B]/5 font-medium text-[#064E3B]/70 border-t border-[#064E3B]/5">Source Mail</div>
                    <div className="px-4 py-3 bg-[#064E3B]/[0.02] border-t border-[#064E3B]/5">
                      {emailHtml ? (
                        <iframe 
                          srcDoc={emailHtml}
                          className="w-full h-[400px] rounded-md border border-[#064E3B]/10 bg-white shadow-sm"
                          sandbox="allow-same-origin"
                          title="Source Email"
                        />
                      ) : (
                        <div className="w-full max-h-[400px] overflow-y-auto rounded-md border border-[#064E3B]/10 bg-white p-4 shadow-sm">
                          <p className="text-[13px] text-[#064E3B]/80 leading-relaxed whitespace-pre-wrap break-words font-mono">
                            {emailText}
                          </p>
                        </div>
                      )}
                    </div>
                  </div>
                )}
              </div>
            </div>

          </div>

          {/* Action Zone at bottom */}
          <div className="mt-8 pt-4 border-t border-[#064E3B]/10 flex justify-end gap-3">
            <Button
              variant="outline"
              className="text-[13px] h-9 border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5 font-medium"
              onClick={handleDismiss}
            >
              <Trash2 className="w-3.5 h-3.5 mr-2" /> Dismiss Notification
            </Button>
          </div>

        </div>
      </aside>
  );
}
