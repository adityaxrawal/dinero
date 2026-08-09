import { cn } from '@/lib/utils';

/**
 * How wrong the parser's guess probably is.
 *
 * Deliberately not `ConfidenceMeter`: everything in this queue scored below the
 * 0.60 threshold, so a 0-to-1 scale collapses every row onto its bottom band and
 * says "Weak" eight times over. What the user needs here is ordering *within*
 * the bad range, and wording about the guess rather than about trustworthiness.
 */
export default function GuessQuality({ confidence }: { confidence: number }) {
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
