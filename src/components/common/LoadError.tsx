/**
 * The one "couldn't load" state (audit Phase 2 2.3). Says what failed, that
 * nothing is lost, and offers Try again. A failed load must never read as an
 * empty list ("All caught up", "No transactions"), which is what several
 * screens used to show.
 */
import { cn } from '@/lib/utils';

export default function LoadError({
  what,
  onRetry,
  className,
}: {
  /** Mid-sentence noun phrase: "the review queue", "recent activity". */
  what: string;
  onRetry?: () => void;
  className?: string;
}) {
  return (
    <div
      role="alert"
      className={cn('flex flex-col items-center justify-center gap-2 p-6 text-center', className)}
    >
      <p className="text-[13px] font-semibold text-emerald-ink">Couldn't load {what}</p>
      <p className="text-[12px] text-emerald-ink/85">Your data is safe; it just didn't load.</p>
      {onRetry && (
        <button
          type="button"
          onClick={onRetry}
          className="text-[12px] font-medium text-emerald-ink px-3 py-1 rounded-full border border-emerald-ink/20 hover:bg-emerald-ink/10"
        >
          Try again
        </button>
      )}
    </div>
  );
}
