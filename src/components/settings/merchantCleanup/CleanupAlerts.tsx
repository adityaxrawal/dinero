/**
 * Warnings and errors raised during a cleanup run.
 */
import { AlertTriangle } from 'lucide-react';

/** One alert banner. */
function Banner({ tone, children }: { tone: 'error' | 'warn'; children: React.ReactNode }) {
  const className =
    tone === 'error'
      ? 'border-red-300 bg-red-50 text-red-800'
      : 'border-amber-300 bg-amber-50 text-amber-900';
  return (
    <div className={`mb-4 p-4 rounded-xl border text-sm flex items-start gap-2 ${className}`}>
      <AlertTriangle className="w-4 h-4 mt-0.5 shrink-0" />
      <span>{children}</span>
    </div>
  );
}

/** Warnings and errors raised during a run. */
export default function CleanupAlerts({
  error,
  blocked,
  noModel,
  totalRamGb,
}: {
  error: string | null;
  blocked: boolean;
  noModel: boolean;
  totalRamGb: number | undefined;
}) {
  return (
    <>
      {error && <Banner tone="error">{error}</Banner>}

      {blocked ? (
        <Banner tone="warn">
          On-device AI needs more memory than this Mac has ({totalRamGb?.toFixed(1)} GB). Merchant
          cleanup is unavailable here.
        </Banner>
      ) : (
        noModel && (
          <Banner tone="warn">
            No AI model is downloaded yet, so there is nothing to read your emails with. Pick one
            under <strong className="font-semibold">Local LLM Configuration</strong> below, then come
            back here.
          </Banner>
        )
      )}
    </>
  );
}
