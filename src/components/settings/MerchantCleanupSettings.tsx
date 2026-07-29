import { useState, useEffect, useCallback } from 'react';
import { Sparkles, Loader2, Undo2, XCircle, AlertTriangle, FileWarning } from 'lucide-react';
import {
  API,
  type MerchantCleanupPreview,
  type MerchantCleanupProgress,
} from '@/lib/ipc';
import { useIpcListen } from '@/hooks/useIpcListen';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

/**
 * Issue #12: "Normalize with LLM".
 *
 * Surfaces the transactions whose merchant name the parser is least sure
 * about and lets the user hand them to the on-device model, which reads the
 * original email and returns the real merchant plus a category.
 *
 * The undo affordance is deliberately prominent: this rewrites merchant names
 * across potentially thousands of rows at once, and a bad run must be one
 * click to put back.
 */
/**
 * Rough wall-clock estimate. Assumes ~3s per transaction spread over the
 * sidecar's ~6 concurrent slots — deliberately coarse, since the real rate
 * depends on the chosen model and the Mac. Only used to set expectations
 * before a long run, never to drive logic.
 */
function estimateMinutes(count: number): string {
  const minutes = Math.ceil((count * 3) / 6 / 60);
  if (minutes < 1) return 'under a minute';
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

export default function MerchantCleanupSettings() {
  const [preview, setPreview] = useState<MerchantCleanupPreview | null>(null);
  const [progress, setProgress] = useState<MerchantCleanupProgress | null>(null);
  const [isStarting, setIsStarting] = useState(false);
  const [lastRunId, setLastRunId] = useState<string | null>(null);
  const [isReverting, setIsReverting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadPreview = useCallback(() => {
    API.merchantCleanup
      .preview()
      .then(setPreview)
      .catch((err) => setError(err?.message ?? String(err)));
  }, []);

  useEffect(loadPreview, [loadPreview]);

  useIpcListen<MerchantCleanupProgress>('merchant_cleanup_progress', (payload) => {
    setProgress(payload);
    if (payload.status !== 'running') {
      // The queue is derived from confidence, so refreshing after a run
      // shows exactly what is left rather than a stale count.
      setLastRunId(payload.applied > 0 ? payload.run_id : null);
      loadPreview();
    }
  });

  const isRunning = progress?.status === 'running' || preview?.running === true;

  const handleStart = async () => {
    setError(null);
    setIsStarting(true);
    try {
      const runId = await API.merchantCleanup.start();
      setProgress({
        run_id: runId,
        processed: 0,
        total: preview?.candidate_count ?? 0,
        applied: 0,
        current_merchant: null,
        status: 'running',
      });
    } catch (err: unknown) {
      const e = err as { message?: string };
      setError(e?.message ?? String(err));
    } finally {
      setIsStarting(false);
    }
  };

  const handleCancel = async () => {
    try {
      await API.merchantCleanup.cancel();
    } catch (err: unknown) {
      const e = err as { message?: string };
      setError(e?.message ?? String(err));
    }
  };

  const handleRevert = async () => {
    if (!lastRunId) return;
    if (
      !confirm(
        'Undo this cleanup run? Every merchant name, category and entity link it changed goes ' +
          'back to what it was, and the extraction rules it learned are retired.'
      )
    )
      return;
    setIsReverting(true);
    try {
      const n = await API.merchantCleanup.revert(lastRunId);
      setLastRunId(null);
      setProgress(null);
      loadPreview();
      alert(`Reverted ${n} correction${n === 1 ? '' : 's'}.`);
    } catch (err: unknown) {
      const e = err as { message?: string };
      setError(e?.message ?? String(err));
    } finally {
      setIsReverting(false);
    }
  };

  const pct =
    progress && progress.total > 0
      ? Math.round((progress.processed / progress.total) * 100)
      : 0;

  const noEvidenceCount = preview?.samples.filter((s) => !s.has_evidence).length ?? 0;

  return (
    <section>
      <div className="mb-6">
        <h2 className="text-xl font-bold flex items-center gap-2">
          <Sparkles className="w-5 h-5" /> Merchant Names &amp; Categories
        </h2>
        <p className="text-sm mt-1 text-[#064E3B]/70">
          Some transactions end up with a merchant name the parser guessed badly — a truncated
          brand, a payment gateway, or a fragment of the email. This hands those to the on-device
          AI, which reads the original email and fills in the real merchant name and a category.
          It also teaches the parser, so the next scan gets that email shape right on its own.
          Nothing leaves your Mac, and every change can be undone.
        </p>
      </div>

      {error && (
        <div className="mb-4 p-4 rounded-xl border border-red-300 bg-red-50 text-sm text-red-800 flex items-start gap-2">
          <AlertTriangle className="w-4 h-4 mt-0.5 shrink-0" />
          <span>{error}</span>
        </div>
      )}

      {preview && !preview.llm_eligible && (
        <div className="mb-4 p-4 rounded-xl border border-amber-300 bg-amber-50 text-sm text-amber-900 flex items-start gap-2">
          <AlertTriangle className="w-4 h-4 mt-0.5 shrink-0" />
          <span>
            On-device AI needs more memory than this Mac has ({preview.total_ram_gb.toFixed(1)} GB).
            Merchant cleanup is unavailable here.
          </span>
        </div>
      )}

      <div className="mb-6 p-5 rounded-xl border bg-[#F8E7C9]/50 border-[#064E3B]/10">
        <div className="flex items-center justify-between flex-wrap gap-3">
          <div>
            <h3 className="font-bold text-[15px] text-[#064E3B]">
              {preview === null
                ? 'Checking your transactions…'
                : preview.candidate_count === 0
                  ? 'Every merchant name looks good'
                  : `${preview.candidate_count} transaction${preview.candidate_count === 1 ? '' : 's'} need attention`}
            </h3>
            <p className="text-xs mt-1 text-[#064E3B]/60">
              {preview && preview.candidate_count > 0
                ? `Worst first, so you can stop early and keep the fixes. Roughly ${estimateMinutes(preview.candidate_count)}.`
                : 'Nothing scored below the confidence threshold.'}
            </p>
          </div>

          <div className="flex items-center gap-2">
            {isRunning ? (
              <Button variant="outline" onClick={handleCancel}>
                <XCircle className="w-4 h-4 mr-2" /> Stop
              </Button>
            ) : (
              <Button
                onClick={handleStart}
                disabled={
                  isStarting ||
                  !preview ||
                  preview.candidate_count === 0 ||
                  !preview.llm_eligible
                }
              >
                {isStarting ? (
                  <Loader2 className="w-4 h-4 mr-2 animate-spin" />
                ) : (
                  <Sparkles className="w-4 h-4 mr-2" />
                )}
                Normalize with AI
              </Button>
            )}

            {lastRunId && !isRunning && (
              <Button variant="outline" onClick={handleRevert} disabled={isReverting}>
                {isReverting ? (
                  <Loader2 className="w-4 h-4 mr-2 animate-spin" />
                ) : (
                  <Undo2 className="w-4 h-4 mr-2" />
                )}
                Undo last run
              </Button>
            )}
          </div>
        </div>

        {progress && (
          <div className="mt-4">
            <div className="h-2 rounded-full bg-[#064E3B]/10 overflow-hidden">
              <div
                className={cn(
                  'h-full rounded-full transition-all duration-300',
                  progress.status === 'failed' ? 'bg-red-500' : 'bg-[#064E3B]'
                )}
                style={{ width: `${pct}%` }}
              />
            </div>
            <div className="flex items-center justify-between mt-2 text-xs text-[#064E3B]/70">
              <span>
                {progress.status === 'running'
                  ? progress.current_merchant
                    ? `Reading “${progress.current_merchant}”…`
                    : 'Starting…'
                  : progress.status === 'cancelled'
                    ? 'Stopped'
                    : progress.status === 'failed'
                      ? 'Run failed'
                      : 'Done'}
              </span>
              <span>
                {progress.processed} / {progress.total} · {progress.applied} fixed
              </span>
            </div>
          </div>
        )}
      </div>

      {preview && preview.samples.length > 0 && !isRunning && (
        <div className="rounded-xl border border-[#064E3B]/10 overflow-hidden">
          <div className="px-4 py-2.5 bg-[#064E3B]/[0.04] text-xs font-semibold text-[#064E3B]/70 flex items-center justify-between">
            <span>Lowest confidence first</span>
            <span>Confidence</span>
          </div>
          <ul className="divide-y divide-[#064E3B]/10">
            {preview.samples.map((s) => (
              <li
                key={s.transaction_id}
                className="px-4 py-2.5 flex items-center justify-between gap-3 text-sm"
              >
                <div className="min-w-0">
                  <div className="font-medium text-[#064E3B] truncate">{s.merchant}</div>
                  <div className="text-xs text-[#064E3B]/60 flex items-center gap-1.5">
                    {s.bank_name}
                    {!s.has_evidence && (
                      <span
                        className="inline-flex items-center gap-1 text-amber-700"
                        title="The original email is no longer stored, so this one will be skipped."
                      >
                        <FileWarning className="w-3 h-3" /> no email kept
                      </span>
                    )}
                  </div>
                </div>
                <span
                  className={cn(
                    'text-xs font-mono px-2 py-0.5 rounded-full shrink-0',
                    s.confidence < 0.3
                      ? 'bg-red-100 text-red-800'
                      : 'bg-amber-100 text-amber-800'
                  )}
                >
                  {Math.round(s.confidence * 100)}%
                </span>
              </li>
            ))}
          </ul>
          {preview.candidate_count > preview.samples.length && (
            <div className="px-4 py-2.5 text-xs text-[#064E3B]/60 bg-[#064E3B]/[0.02]">
              …and {preview.candidate_count - preview.samples.length} more.
              {noEvidenceCount > 0 &&
                ' Transactions whose email is no longer stored will be skipped.'}
            </div>
          )}
        </div>
      )}
    </section>
  );
}
