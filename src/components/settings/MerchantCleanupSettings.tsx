import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import {
  Sparkles,
  Loader2,
  Undo2,
  XCircle,
  AlertTriangle,
  FileWarning,
  Cpu,
  Landmark,
  ListChecks,
  Timer,
  CheckCircle2,
  MinusCircle,
  ChevronRight,
  History,
  ArrowRight,
} from 'lucide-react';
import {
  API,
  type MerchantCleanupPreview,
  type MerchantCleanupProgress,
  type MerchantCleanupRun,
  type MerchantCleanupSample,
  type LlmModelInfo,
} from '@/lib/ipc';
import { useIpcListen } from '@/hooks/useIpcListen';
import { useNowTicker } from '@/hooks/useNowTicker';
import { Button } from '@/components/ui/button';
import { toast } from '@/hooks/use-toast';
import { cn } from '@/lib/utils';
import { ConfirmDialog, RelativeDate, StatStrip, StatTile } from './SettingsPrimitives';

/**
 * Issue #12: "Normalize with LLM".
 *
 * Surfaces the transactions whose merchant name the parser is least sure
 * about and lets the user hand them to the on-device model, which reads the
 * original email and returns the real merchant plus a category.
 *
 * Three things this panel has to answer at all times, because a run is long and
 * silent: what state is it in, is it actually working, and how do I take it
 * back. So the run reports measured rate and ETA rather than a static estimate,
 * shows the model's answers as they land rather than only a counter, and reads
 * its undo affordance out of the database — `merchant_llm_corrections` is the
 * run record, so a window reload can no longer strand a revertible run.
 */

/**
 * Rough wall-clock estimate for the *idle* state, before any real rate exists.
 * Assumes ~3s per transaction spread over the sidecar's ~6 concurrent slots —
 * deliberately coarse, since the real rate depends on the chosen model and the
 * Mac. Once a run starts, the measured rate replaces this everywhere.
 */
function estimateMinutes(count: number): string {
  const minutes = Math.ceil((count * 3) / 6 / 60);
  if (minutes < 1) return 'under a minute';
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

/** `m:ss` under an hour, then `h:mm:ss`. */
function formatClock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const s = total % 60;
  const m = Math.floor(total / 60) % 60;
  const h = Math.floor(total / 3600);
  const pad = (n: number) => String(n).padStart(2, '0');
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return '—';
  if (seconds < 60) return `${Math.ceil(seconds)}s`;
  const minutes = Math.ceil(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

function formatAmount(amount: number | null, currency: string | null): string | null {
  if (amount === null) return null;
  try {
    return new Intl.NumberFormat(undefined, {
      style: 'currency',
      currency: currency ?? 'INR',
      maximumFractionDigits: 2,
    }).format(amount);
  } catch {
    return `${currency ?? ''} ${amount.toFixed(2)}`.trim();
  }
}

/** One line in the live feed of what the run just did. */
type FeedEntry = {
  key: number;
  before: string;
  after: string | null;
  category: string | null;
};

const FEED_LENGTH = 8;

/**
 * How wrong the parser's guess probably is.
 *
 * Deliberately not `ConfidenceMeter`: everything in this queue scored below the
 * 0.60 threshold, so a 0-to-1 scale collapses every row onto its bottom band and
 * says "Weak" eight times over. What the user needs here is ordering *within*
 * the bad range, and wording about the guess rather than about trustworthiness.
 */
function GuessQuality({ confidence }: { confidence: number }) {
  const [label, tone] =
    confidence < 0.2
      ? (['Almost certainly wrong', 'text-red-700'] as const)
      : confidence < 0.35
        ? (['Probably wrong', 'text-red-600'] as const)
        : (['Doubtful', 'text-amber-700'] as const);

  return (
    <span
      className={cn('inline-flex items-center gap-1.5 shrink-0', tone)}
      title={`The parser was ${Math.round(confidence * 100)}% sure of this name`}
    >
      <span className="text-[11px] font-semibold">{label}</span>
      <span className="text-[11px] tabular-nums opacity-60">{Math.round(confidence * 100)}%</span>
    </span>
  );
}

function QueueRow({ sample }: { sample: MerchantCleanupSample }) {
  const amount = formatAmount(sample.amount, sample.currency);
  return (
    <li className="px-4 py-2.5 flex items-center justify-between gap-3">
      <div className="min-w-0">
        <div className="font-mono text-[13px] font-medium text-[#064E3B] truncate">
          {sample.merchant}
        </div>
        <div className="text-[11px] text-[#064E3B]/55 flex items-center gap-1.5 flex-wrap mt-0.5">
          {amount && (
            <span className={sample.direction === 'credit' ? 'text-emerald-700' : undefined}>
              {sample.direction === 'credit' ? '+' : ''}
              {amount}
            </span>
          )}
          {sample.event_time && (
            <>
              <span className="text-[#064E3B]/25">·</span>
              <RelativeDate iso={sample.event_time} />
            </>
          )}
          {!sample.has_evidence && (
            <span
              className="inline-flex items-center gap-1 text-amber-700"
              title="The original email is no longer stored, so this one will be skipped."
            >
              <FileWarning className="w-3 h-3" /> no email kept
            </span>
          )}
        </div>
      </div>
      <GuessQuality confidence={sample.confidence} />
    </li>
  );
}

function RunHistoryRow({
  run,
  onUndoRun,
  onUndoChange,
  busyId,
}: {
  run: MerchantCleanupRun;
  onUndoRun: (run: MerchantCleanupRun) => void;
  onUndoChange: (correctionId: string) => void;
  busyId: string | null;
}) {
  const [open, setOpen] = useState(false);

  return (
    <div className="rounded-xl border border-[#064E3B]/10 bg-white overflow-hidden">
      <div className="px-4 py-3 flex items-center gap-3 flex-wrap">
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          className="flex items-center gap-2 min-w-0 flex-1 text-left"
        >
          <ChevronRight
            className={cn(
              'w-4 h-4 shrink-0 text-[#064E3B]/50 transition-transform',
              open && 'rotate-90'
            )}
          />
          <span className="font-semibold text-[13px] text-[#064E3B] shrink-0">
            <RelativeDate iso={run.started_at} />
          </span>
          <span className="text-[12px] text-[#064E3B]/60 truncate">
            {run.applied} still applied
            {run.reverted > 0 && ` · ${run.reverted} undone`}
            {run.banks.length > 0 && ` · ${run.banks.join(', ')}`}
          </span>
        </button>

        {run.applied > 0 ? (
          <Button
            variant="outline"
            size="sm"
            onClick={() => onUndoRun(run)}
            disabled={busyId === run.run_id}
            className="shrink-0 border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5"
          >
            {busyId === run.run_id ? (
              <Loader2 className="w-3.5 h-3.5 animate-spin" />
            ) : (
              <Undo2 className="w-3.5 h-3.5" />
            )}
            <span className="ml-1.5">Undo run</span>
          </Button>
        ) : (
          <span className="text-[11px] font-semibold uppercase tracking-wide text-[#064E3B]/40 shrink-0">
            Already undone
          </span>
        )}
      </div>

      {open && (
        <ul className="border-t border-[#064E3B]/[0.07] divide-y divide-[#064E3B]/[0.07]">
          {run.changes.map((c) => (
            <li
              key={c.correction_id}
              className={cn(
                'px-4 py-2.5 flex items-center justify-between gap-3',
                c.reverted && 'opacity-50'
              )}
            >
              <div className="min-w-0 flex items-center gap-2 flex-wrap text-[12px]">
                <span className="font-mono text-[#064E3B]/60 line-through decoration-[#064E3B]/30">
                  {c.previous_merchant ?? '—'}
                </span>
                <ArrowRight className="w-3 h-3 shrink-0 text-[#064E3B]/35" />
                <span className="font-semibold text-[#064E3B]">{c.new_merchant ?? '—'}</span>
                {c.category && (
                  <span className="text-[10px] font-semibold px-1.5 py-0.5 rounded bg-[#064E3B]/[0.07] text-[#064E3B]/70">
                    {c.category}
                  </span>
                )}
              </div>
              {c.reverted ? (
                <span className="text-[11px] text-[#064E3B]/45 shrink-0">undone</span>
              ) : (
                <button
                  type="button"
                  onClick={() => onUndoChange(c.correction_id)}
                  disabled={busyId === c.correction_id}
                  className="shrink-0 text-[11px] font-semibold text-[#064E3B]/60 hover:text-[#064E3B] underline underline-offset-2"
                >
                  {busyId === c.correction_id ? 'undoing…' : 'undo'}
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export default function MerchantCleanupSettings() {
  const [preview, setPreview] = useState<MerchantCleanupPreview | null>(null);
  const [progress, setProgress] = useState<MerchantCleanupProgress | null>(null);
  const [runs, setRuns] = useState<MerchantCleanupRun[]>([]);
  const [feed, setFeed] = useState<FeedEntry[]>([]);
  const [isStarting, setIsStarting] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [activeModel, setActiveModel] = useState<LlmModelInfo | null>(null);
  const [pendingRun, setPendingRun] = useState<MerchantCleanupRun | null>(null);
  const [showQueue, setShowQueue] = useState(false);
  /** Wall-clock start of the current run, for elapsed and measured rate. */
  const startedAt = useRef<number | null>(null);
  const feedKey = useRef(0);

  const loadPreview = useCallback(() => {
    API.merchantCleanup
      .preview()
      .then(setPreview)
      .catch((err) => setError(err?.message ?? String(err)));
  }, []);

  const loadRuns = useCallback(() => {
    API.merchantCleanup
      .runs()
      .then(setRuns)
      .catch((err) => setError(err?.message ?? String(err)));
  }, []);

  useEffect(() => {
    loadPreview();
    loadRuns();
    // The model name is what makes "on-device AI" concrete; without it the user
    // cannot tell what is about to read their mail.
    Promise.all([API.llm.getActiveModel(), API.llm.getAvailableModels()])
      .then(([id, models]) => setActiveModel(models.find((m) => m.id === id) ?? null))
      .catch(() => setActiveModel(null));
  }, [loadPreview, loadRuns]);

  useIpcListen<MerchantCleanupProgress>('merchant_cleanup_progress', (payload) => {
    setProgress(payload);
    if (payload.status === 'running' && startedAt.current === null) {
      startedAt.current = Date.now();
    }
    if (payload.current_merchant) {
      feedKey.current += 1;
      const entry: FeedEntry = {
        key: feedKey.current,
        before: payload.current_merchant,
        after: payload.resolved_merchant,
        category: payload.resolved_category,
      };
      setFeed((prev) => [entry, ...prev].slice(0, FEED_LENGTH));
    }
    if (payload.status !== 'running') {
      startedAt.current = null;
      // The queue is derived from confidence, so refreshing after a run
      // shows exactly what is left rather than a stale count.
      loadPreview();
      loadRuns();
    }
  });

  const isRunning = progress?.status === 'running' || preview?.running === true;
  const now = useNowTicker(isRunning);

  const handleStart = async () => {
    setError(null);
    setIsStarting(true);
    setFeed([]);
    startedAt.current = Date.now();
    try {
      const runId = await API.merchantCleanup.start();
      setProgress({
        run_id: runId,
        processed: 0,
        total: preview?.candidate_count ?? 0,
        applied: 0,
        skipped: 0,
        current_merchant: null,
        bank_name: null,
        resolved_merchant: null,
        resolved_category: null,
        status: 'running',
      });
    } catch (err: unknown) {
      const e = err as { message?: string };
      setError(e?.message ?? String(err));
      startedAt.current = null;
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

  const revertRun = async (run: MerchantCleanupRun) => {
    setBusyId(run.run_id);
    setError(null);
    try {
      const n = await API.merchantCleanup.revert(run.run_id);
      loadPreview();
      loadRuns();
      setProgress(null);
      toast({
        title: `Undid ${n} correction${n === 1 ? '' : 's'}`,
        description:
          'Every merchant name, category and entity link that run changed is back, and the rules it taught are retired.',
      });
    } catch (err: unknown) {
      const e = err as { message?: string };
      setError(e?.message ?? String(err));
    } finally {
      setBusyId(null);
    }
  };

  const revertChange = async (correctionId: string) => {
    setBusyId(correctionId);
    setError(null);
    try {
      await API.merchantCleanup.revertCorrection(correctionId);
      loadPreview();
      loadRuns();
      toast({ title: 'Correction undone' });
    } catch (err: unknown) {
      const e = err as { message?: string };
      setError(e?.message ?? String(err));
    } finally {
      setBusyId(null);
    }
  };

  const pct =
    progress && progress.total > 0 ? Math.round((progress.processed / progress.total) * 100) : 0;

  /** Measured throughput, which is the only honest basis for an ETA. */
  const live = useMemo(() => {
    if (!progress || startedAt.current === null) return null;
    const elapsedMs = now - startedAt.current;
    const perMin = elapsedMs > 0 ? (progress.processed / elapsedMs) * 60000 : 0;
    const remaining = progress.total - progress.processed;
    return {
      elapsed: formatClock(elapsedMs),
      perMin: perMin >= 0.1 ? perMin.toFixed(1) : '—',
      eta: perMin > 0 ? formatDuration((remaining / perMin) * 60) : '—',
    };
  }, [progress, now]);

  const isFinished = progress !== null && progress.status !== 'running' && progress.processed > 0;
  const finishedRun = isFinished ? runs.find((r) => r.run_id === progress.run_id) : undefined;

  const noModel = activeModel === null;
  const blocked = preview !== null && !preview.llm_eligible;
  const worst = preview?.samples[0];

  return (
    <section>
      <div className="mb-5">
        <h2 className="text-xl font-bold flex items-center gap-2">
          <Sparkles className="w-5 h-5" /> Merchant Names &amp; Categories
        </h2>
        <p className="text-sm mt-1 text-[#064E3B]/70 leading-relaxed">
          Some transactions end up with a merchant name the parser guessed badly — a truncated
          brand, a payment gateway, or a fragment of the email. This hands those to the on-device
          AI, which reads the original email and fills in the real merchant name and a category. It
          also teaches the parser, so the next scan gets that email shape right on its own. Nothing
          leaves your Mac, and every change can be undone.
        </p>
      </div>

      {error && (
        <div className="mb-4 p-4 rounded-xl border border-red-300 bg-red-50 text-sm text-red-800 flex items-start gap-2">
          <AlertTriangle className="w-4 h-4 mt-0.5 shrink-0" />
          <span>{error}</span>
        </div>
      )}

      {blocked ? (
        <div className="mb-4 p-4 rounded-xl border border-amber-300 bg-amber-50 text-sm text-amber-900 flex items-start gap-2">
          <AlertTriangle className="w-4 h-4 mt-0.5 shrink-0" />
          <span>
            On-device AI needs more memory than this Mac has ({preview?.total_ram_gb.toFixed(1)}{' '}
            GB). Merchant cleanup is unavailable here.
          </span>
        </div>
      ) : (
        noModel && (
          <div className="mb-4 p-4 rounded-xl border border-amber-300 bg-amber-50 text-sm text-amber-900 flex items-start gap-2">
            <AlertTriangle className="w-4 h-4 mt-0.5 shrink-0" />
            <span>
              No AI model is downloaded yet, so there is nothing to read your emails with. Pick one
              under <strong className="font-semibold">Local LLM Configuration</strong> below, then
              come back here.
            </span>
          </div>
        )
      )}

      {preview !== null && preview.candidate_count > 0 && (
        <div className="mb-4">
          <StatStrip>
            <StatTile
              icon={<ListChecks />}
              label="Need attention"
              value={preview.candidate_count}
              hint="scored below the threshold"
            />
            <StatTile
              icon={<FileWarning />}
              label="Will be skipped"
              value={preview.no_evidence_count}
              hint="email no longer kept"
              tone={preview.no_evidence_count > 0 ? 'warn' : 'default'}
            />
            <StatTile
              icon={<Landmark />}
              label="Banks affected"
              value={preview.by_bank.length}
              hint={preview.by_bank[0]?.bank_name}
            />
            <StatTile
              icon={<Timer />}
              label="Estimated time"
              value={estimateMinutes(preview.candidate_count - preview.no_evidence_count)}
              hint={activeModel ? activeModel.name : 'no model selected'}
            />
          </StatStrip>
        </div>
      )}

      {/* ── The action card: idle, running, or finished ── */}
      <div
        className={cn(
          'mb-5 p-5 rounded-xl border transition-colors',
          isRunning
            ? 'bg-[#064E3B]/[0.06] border-[#064E3B]/30'
            : 'bg-[#F8E7C9]/50 border-[#064E3B]/10'
        )}
      >
        <div className="flex items-start justify-between flex-wrap gap-3">
          <div className="min-w-0">
            <h3 className="font-bold text-[15px] text-[#064E3B]">
              {preview === null
                ? 'Checking your transactions…'
                : isRunning
                  ? progress?.bank_name
                    ? `Reading ${progress.bank_name} alerts…`
                    : 'Starting the on-device AI…'
                  : isFinished
                    ? progress.status === 'cancelled'
                      ? 'Stopped early — the fixes so far are kept'
                      : progress.status === 'failed'
                        ? 'Run failed'
                        : 'Cleanup finished'
                    : preview.candidate_count === 0
                      ? 'Every merchant name looks good'
                      : `${preview.candidate_count} merchant name${preview.candidate_count === 1 ? '' : 's'} Dinero isn't sure about`}
            </h3>
            <p className="text-[12px] mt-1 text-[#064E3B]/60 leading-relaxed max-w-xl">
              {isRunning
                ? 'Worst first — stop any time and everything fixed so far is kept.'
                : isFinished
                  ? `Fixed ${progress.applied} of ${progress.processed} read${progress.skipped > 0 ? `, ${progress.skipped} skipped` : ''}. ${preview && preview.candidate_count > 0 ? `${preview.candidate_count} still to go.` : 'Nothing left in the queue.'}`
                  : preview && preview.candidate_count > 0
                    ? 'These came out of the email as a gateway code or a fragment of a sentence. Worst first, so you can stop early and keep the fixes.'
                    : 'Nothing scored below the confidence threshold.'}
            </p>
          </div>

          <div className="flex items-center gap-2 shrink-0">
            {isRunning ? (
              <Button
                variant="outline"
                onClick={handleCancel}
                className="border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5"
              >
                <XCircle className="w-4 h-4 mr-2" /> Stop
              </Button>
            ) : (
              <Button
                variant="accent"
                onClick={handleStart}
                disabled={
                  isStarting ||
                  !preview ||
                  preview.candidate_count === 0 ||
                  !preview.llm_eligible ||
                  noModel
                }
              >
                {isStarting ? (
                  <Loader2 className="w-4 h-4 mr-2 animate-spin" />
                ) : (
                  <Sparkles className="w-4 h-4 mr-2" />
                )}
                {isFinished && preview && preview.candidate_count > 0
                  ? 'Continue'
                  : 'Normalize with AI'}
              </Button>
            )}

            {finishedRun && finishedRun.applied > 0 && !isRunning && (
              <Button
                variant="outline"
                onClick={() => setPendingRun(finishedRun)}
                disabled={busyId === finishedRun.run_id}
                className="border-red-200 text-red-600 hover:bg-red-50 hover:border-red-300"
              >
                {busyId === finishedRun.run_id ? (
                  <Loader2 className="w-4 h-4 mr-2 animate-spin" />
                ) : (
                  <Undo2 className="w-4 h-4 mr-2" />
                )}
                Undo this run
              </Button>
            )}
          </div>
        </div>

        {/* Idle: one concrete example, so "not sure about" is not abstract. */}
        {!isRunning && !isFinished && worst && (
          <div className="mt-4 pt-4 border-t border-[#064E3B]/10 flex items-center gap-3 flex-wrap text-[13px]">
            <span className="text-[11px] font-semibold uppercase tracking-wide text-[#064E3B]/45">
              Worst match
            </span>
            <span className="font-mono text-[#064E3B]">{worst.merchant}</span>
            <ArrowRight className="w-3.5 h-3.5 text-[#064E3B]/35" />
            <span className="text-[#064E3B]/55 italic">read from the original email</span>
            <GuessQuality confidence={worst.confidence} />
          </div>
        )}

        {/* Running / just finished: progress, measured stats, live answers. */}
        {progress && (isRunning || isFinished) && (
          <div className="mt-4">
            <div className="h-1.5 rounded-full bg-[#064E3B]/10 overflow-hidden">
              <div
                className={cn(
                  'h-full rounded-full transition-all duration-300',
                  progress.status === 'failed'
                    ? 'bg-red-500'
                    : progress.status === 'cancelled'
                      ? 'bg-amber-500'
                      : 'bg-[#064E3B]'
                )}
                style={{ width: `${pct}%` }}
              />
            </div>
            <div className="flex items-center justify-between mt-1.5 text-[11px] font-semibold text-[#064E3B]/60 tabular-nums">
              <span>
                {progress.processed} / {progress.total}
              </span>
              <span>{pct}%</span>
            </div>

            <div className="mt-3 grid grid-cols-2 sm:grid-cols-4 gap-x-4 gap-y-2 text-[12px]">
              <span className="flex items-center gap-1.5">
                <CheckCircle2 className="w-3.5 h-3.5 text-emerald-600" />
                <span className="text-[#064E3B]/60">Fixed</span>
                <strong className="font-bold text-[#064E3B] tabular-nums">
                  {progress.applied}
                </strong>
              </span>
              <span className="flex items-center gap-1.5">
                <MinusCircle className="w-3.5 h-3.5 text-[#064E3B]/40" />
                <span className="text-[#064E3B]/60">Skipped</span>
                <strong className="font-bold text-[#064E3B] tabular-nums">
                  {progress.skipped}
                </strong>
              </span>
              {live && isRunning && (
                <>
                  <span className="flex items-center gap-1.5">
                    <Sparkles className="w-3.5 h-3.5 text-[#064E3B]/40" />
                    <span className="text-[#064E3B]/60">Rate</span>
                    <strong className="font-bold text-[#064E3B] tabular-nums">
                      {live.perMin}/min
                    </strong>
                  </span>
                  <span className="flex items-center gap-1.5">
                    <Timer className="w-3.5 h-3.5 text-[#064E3B]/40" />
                    <span className="text-[#064E3B]/60">Left</span>
                    <strong className="font-bold text-[#064E3B] tabular-nums">~{live.eta}</strong>
                  </span>
                </>
              )}
            </div>

            {isRunning && (
              <p className="mt-2 text-[11px] text-[#064E3B]/50">
                {live && <>elapsed {live.elapsed} · </>}
                {activeModel?.name ?? 'on-device model'} · nothing leaves your Mac
              </p>
            )}

            {feed.length > 0 && (
              <ul className="mt-3 pt-3 border-t border-[#064E3B]/10 space-y-1.5">
                {feed.map((f) => (
                  <li key={f.key} className="flex items-center gap-2 text-[12px] animate-fade-in">
                    {f.after ? (
                      <CheckCircle2 className="w-3.5 h-3.5 shrink-0 text-emerald-600" />
                    ) : (
                      <MinusCircle className="w-3.5 h-3.5 shrink-0 text-[#064E3B]/30" />
                    )}
                    <span className="font-mono text-[#064E3B]/55 truncate max-w-[40%]">
                      {f.before}
                    </span>
                    <ArrowRight className="w-3 h-3 shrink-0 text-[#064E3B]/30" />
                    {f.after ? (
                      <>
                        <span className="font-semibold text-[#064E3B] truncate">{f.after}</span>
                        {f.category && (
                          <span className="text-[10px] font-semibold px-1.5 py-0.5 rounded bg-[#064E3B]/[0.07] text-[#064E3B]/70 shrink-0">
                            {f.category}
                          </span>
                        )}
                      </>
                    ) : (
                      <span className="text-[#064E3B]/45 italic">
                        left alone — no email kept, or the answer did not check out
                      </span>
                    )}
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
      </div>

      {/* ── The queue, grouped by bank ── */}
      {preview && preview.by_bank.length > 0 && !isRunning && (
        <div className="mb-5">
          <button
            type="button"
            onClick={() => setShowQueue((v) => !v)}
            className="w-full flex items-center gap-2 text-left"
          >
            <ChevronRight
              className={cn(
                'w-4 h-4 shrink-0 text-[#064E3B]/50 transition-transform',
                showQueue && 'rotate-90'
              )}
            />
            <h3 className="font-bold text-[14px] text-[#064E3B]">What is in the queue</h3>
            <span className="text-[12px] text-[#064E3B]/55">
              {preview.by_bank.length} bank{preview.by_bank.length === 1 ? '' : 's'}
            </span>
          </button>

          <div className="mt-2.5 flex flex-col gap-1.5">
            {preview.by_bank.map((b) => (
              <div key={b.bank_name} className="flex items-center gap-3">
                <span className="text-[12px] font-semibold text-[#064E3B] w-32 shrink-0 truncate">
                  {b.bank_name}
                </span>
                <div className="flex-1 h-1.5 rounded-full bg-[#064E3B]/[0.08] overflow-hidden">
                  <div
                    className="h-full rounded-full bg-[#064E3B]/60"
                    style={{
                      width: `${(b.count / preview.by_bank[0].count) * 100}%`,
                    }}
                  />
                </div>
                <span className="text-[11px] text-[#064E3B]/55 tabular-nums shrink-0 w-24 text-right">
                  {b.count}
                  {b.no_evidence > 0 && (
                    <span className="text-amber-700"> · {b.no_evidence} skip</span>
                  )}
                </span>
              </div>
            ))}
          </div>

          {showQueue && preview.samples.length > 0 && (
            <div className="mt-3 rounded-xl border border-[#064E3B]/10 bg-white overflow-hidden">
              <div className="px-4 py-2 bg-[#064E3B]/[0.04] text-[11px] font-semibold uppercase tracking-wide text-[#064E3B]/60">
                Worst {preview.samples.length} of {preview.candidate_count}
              </div>
              <ul className="divide-y divide-[#064E3B]/[0.07]">
                {preview.samples.map((s) => (
                  <QueueRow key={s.transaction_id} sample={s} />
                ))}
              </ul>
              {preview.candidate_count > preview.samples.length && (
                <div className="px-4 py-2.5 text-[11px] text-[#064E3B]/55 bg-[#064E3B]/[0.02]">
                  …and {preview.candidate_count - preview.samples.length} more. The queue is worked
                  out from confidence each time, so it refreshes itself after every run.
                </div>
              )}
            </div>
          )}
        </div>
      )}

      {/* ── Past runs ── */}
      {runs.length > 0 && (
        <div className="mt-6 pt-6 border-t border-[#064E3B]/10">
          <h3 className="font-bold text-[15px] text-[#064E3B] flex items-center gap-2">
            <History className="w-4 h-4" /> Past runs
          </h3>
          <p className="text-[13px] mt-1 mb-3 text-[#064E3B]/65 leading-relaxed max-w-2xl">
            Every run stays undoable — as a whole or one merchant at a time. Undoing also retires
            the extraction rules that run taught.
          </p>
          <div className="flex flex-col gap-2">
            {runs.map((r) => (
              <RunHistoryRow
                key={r.run_id}
                run={r}
                onUndoRun={setPendingRun}
                onUndoChange={revertChange}
                busyId={busyId}
              />
            ))}
          </div>
        </div>
      )}

      {preview !== null && preview.candidate_count === 0 && runs.length === 0 && (
        <div className="p-5 rounded-xl border border-dashed border-[#064E3B]/15 bg-[#F8E7C9]/40 flex items-start gap-2.5">
          <Cpu className="w-4 h-4 mt-0.5 shrink-0 text-[#064E3B]/50" />
          <p className="text-[13px] text-[#064E3B]/65 leading-relaxed">
            Nothing to clean up. Every merchant name currently scores above the confidence
            threshold, so there is nothing worth spending inference on.
          </p>
        </div>
      )}

      <ConfirmDialog
        open={pendingRun !== null}
        onOpenChange={(open) => !open && setPendingRun(null)}
        icon={<Undo2 className="w-5 h-5" aria-hidden="true" />}
        title="Undo this cleanup run?"
        description={
          pendingRun
            ? `Every merchant name, category and entity link those ${pendingRun.applied} correction${pendingRun.applied === 1 ? '' : 's'} changed goes back to what it was, and the extraction rules the run learned are retired.`
            : ''
        }
        confirmLabel="Undo run"
        onConfirm={() => pendingRun && void revertRun(pendingRun)}
      />
    </section>
  );
}
