/**
 * Indicates how confident the model was in a proposed merchant name.
 *
 * Surfacing confidence is what lets the user scrutinise the weak guesses rather
 * than reviewing every change equally.
 */
import { cn } from '@/lib/utils';

/**
 * Shows the model's confidence in a proposed merchant name.
 *
 * Surfacing confidence lets the user scrutinise the weak guesses rather than
 * reviewing every change equally.
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
