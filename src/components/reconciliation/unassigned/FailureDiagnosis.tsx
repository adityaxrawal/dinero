/**
 * Explains in plain language why a transaction could not be attributed.
 */
import { AlertTriangle, RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import type { UnassignedTransactionRecord } from '@/lib/ipc';
import { cn } from '@/lib/utils';
import { buildChecks, extractionBadge, reasonGuidance } from './diagnostics';
import DiagnosticChecklist from './DiagnosticChecklist';

/** Explains in plain language why attribution failed. */
export default function FailureDiagnosis({
  record,
  onRecheck,
  isRechecking = false,
}: {
  record: UnassignedTransactionRecord;
  /** Offered for a missing instrument (current-issues #3); omit to hide. */
  onRecheck?: () => void;
  isRechecking?: boolean;
}) {
  const offerRecheck =
    onRecheck !== undefined &&
    record.can_recheck === true &&
    record.reason === 'gate3_failed:missing_instrument';
  const { engineLabel, confidenceLabel, confidenceBadgeStyle } = extractionBadge(record);
  const { description, tip } = reasonGuidance(record.reason);

  return (
    <div className="bg-white/70 rounded-xl p-5 border border-red-200/80 shadow-sm space-y-4">
      <div className="flex items-center justify-between flex-wrap gap-2 pb-3 border-b border-red-100">
        <h3 className="text-sm font-semibold text-red-900 flex items-center gap-2">
          <AlertTriangle className="w-4 h-4 text-red-600" /> Why did this fail?
        </h3>

        <div className="flex items-center gap-2 flex-wrap">
          <span className="text-[11px] font-medium px-2 py-0.5 rounded-full bg-emerald-ink/10 text-emerald-ink">
            {engineLabel}
          </span>
          <span
            className={cn(
              'text-[11px] font-bold px-2 py-0.5 rounded-full border',
              confidenceBadgeStyle
            )}
          >
            {confidenceLabel}
          </span>
        </div>
      </div>

      <p className="text-[13px] text-emerald-ink/90 leading-relaxed font-medium">{description}</p>

      <DiagnosticChecklist checks={buildChecks(record)} />

      <div className="bg-red-50/60 rounded-lg p-3 text-xs text-red-900/90 font-medium border border-red-100">
        <strong>Tip:</strong> {tip}
      </div>

      {offerRecheck && (
        <div className="flex items-center justify-between gap-3 rounded-lg p-3 bg-emerald-ink/5 border border-emerald-ink/15">
          <p className="text-xs text-emerald-ink">
            Learned a pattern for this bank since? Re-check runs your learned patterns again, then
            the local AI.
          </p>
          <Button
            size="sm"
            className="h-8 text-[12px] bg-emerald-ink text-champagne hover:bg-emerald-ink/90 shrink-0"
            onClick={onRecheck}
            disabled={isRechecking}
          >
            <RefreshCw className="w-3.5 h-3.5 mr-1.5" /> Re-check
          </Button>
        </div>
      )}
    </div>
  );
}
