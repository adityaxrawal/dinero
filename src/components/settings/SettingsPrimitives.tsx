import type { ReactNode } from 'react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

/**
 * The pieces both AI-facing Settings sections need. Extracted only because both
 * use all of them — there is no third consumer and no attempt to generalise
 * beyond what those two panels ask for.
 */

/** One number with a label, for the strip at the top of a section. */
export function StatTile({
  icon,
  label,
  value,
  hint,
  tone = 'default',
}: {
  icon?: ReactNode;
  label: string;
  value: ReactNode;
  hint?: string;
  tone?: 'default' | 'good' | 'warn';
}) {
  return (
    <div
      className={cn(
        'rounded-xl border px-3.5 py-3 min-w-0',
        tone === 'good'
          ? 'bg-emerald-500/[0.07] border-emerald-600/25'
          : tone === 'warn'
            ? 'bg-amber-500/[0.08] border-amber-600/25'
            : 'bg-white/70 border-[#064E3B]/10'
      )}
    >
      {/* Labels and hints wrap rather than truncate: four tiles across a
          max-w-3xl column leaves ~180px each, which clips anything longer than
          one word. The grid equalises heights, so wrapping costs nothing. */}
      <div className="flex items-start gap-1.5 text-[11px] font-semibold uppercase tracking-wide leading-tight text-[#064E3B]/55">
        {icon && (
          <span className="shrink-0 mt-px text-[#064E3B]/45 [&_svg]:w-3.5 [&_svg]:h-3.5">
            {icon}
          </span>
        )}
        <span>{label}</span>
      </div>
      <div
        className={cn(
          'mt-1 text-[19px] font-bold leading-tight tabular-nums truncate',
          tone === 'good'
            ? 'text-emerald-700'
            : tone === 'warn'
              ? 'text-amber-700'
              : 'text-[#064E3B]'
        )}
      >
        {value}
      </div>
      {hint && <div className="mt-0.5 text-[11px] leading-snug text-[#064E3B]/50">{hint}</div>}
    </div>
  );
}

/** Grid wrapper for a row of {@link StatTile}s. */
export function StatStrip({ children }: { children: ReactNode }) {
  return <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">{children}</div>;
}

const CONFIDENCE_BANDS = [
  { min: 0.9, label: 'Reliable', dots: 5, className: 'text-emerald-700' },
  { min: 0.7, label: 'Holding up', dots: 4, className: 'text-[#064E3B]' },
  { min: 0.5, label: 'Unproven', dots: 3, className: 'text-amber-700' },
  { min: 0, label: 'Weak', dots: 2, className: 'text-red-700' },
] as const;

/**
 * Confidence as a word plus a five-dot meter, with the exact figure on hover.
 *
 * A column of "95% / 90% / 80%" tells a non-technical reader nothing and makes
 * a list of genuinely different rules look like repeated rows. The number is
 * still there for anyone debugging — it moved to the tooltip, not away.
 */
export function ConfidenceMeter({ value, className }: { value: number; className?: string }) {
  const band = CONFIDENCE_BANDS.find((b) => value >= b.min) ?? CONFIDENCE_BANDS[3];
  const pct = Math.round(value * 100);

  return (
    <span
      className={cn('inline-flex items-center gap-1.5 shrink-0', band.className, className)}
      title={`${pct}% confidence`}
    >
      <span className="flex items-center gap-[3px]" aria-hidden="true">
        {[0, 1, 2, 3, 4].map((i) => (
          <span
            key={i}
            className={cn(
              'w-[5px] h-[5px] rounded-full',
              i < band.dots ? 'bg-current' : 'bg-current opacity-20'
            )}
          />
        ))}
      </span>
      <span className="text-[11px] font-semibold">{band.label}</span>
      <span className="sr-only">({pct}% confidence)</span>
    </span>
  );
}

/**
 * "2 hours ago" up to a week, then "12 Jul". Full timestamp on hover.
 *
 * `Intl` handles both halves, so this needs no date library.
 */
export function RelativeDate({ iso, className }: { iso: string | null; className?: string }) {
  if (!iso) return <span className={className}>unknown date</span>;

  // SQLite writes "YYYY-MM-DD HH:MM:SS" in UTC with no zone marker; Safari
  // parses that as invalid, so normalise before handing it to Date.
  const parsed = new Date(/[TZ]/.test(iso) ? iso : `${iso.replace(' ', 'T')}Z`);
  if (Number.isNaN(parsed.getTime())) return <span className={className}>unknown date</span>;

  // Reading the clock during render is the whole point of a relative date, and
  // this is a leaf with no memoization — it re-reads on every render of its
  // parent, which is exactly the intended behaviour. Hoisting "now" into state
  // isn't possible here without restructuring the two early returns above.
  // eslint-disable-next-line react-hooks/purity
  const diffMs = Date.now() - parsed.getTime();
  const label = formatRelative(diffMs, parsed);

  return (
    <span className={className} title={parsed.toLocaleString()}>
      {label}
    </span>
  );
}

/**
 * In-app replacement for the native `confirm()` these panels used to call.
 *
 * Both sections ask the same shape of question four times over — "this undoes
 * something, are you sure" — so the wiring lives here rather than twice.
 */
export function ConfirmDialog({
  open,
  onOpenChange,
  icon,
  title,
  description,
  confirmLabel,
  cancelLabel = 'Keep it',
  destructive = true,
  onConfirm,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  icon?: ReactNode;
  title: string;
  description: ReactNode;
  confirmLabel: string;
  cancelLabel?: string;
  destructive?: boolean;
  onConfirm: () => void;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[460px] bg-[#F8E7C9] border-[#064E3B]/20 text-[#064E3B]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2 text-[#064E3B]">
            {icon}
            {title}
          </DialogTitle>
          <DialogDescription className="text-[13px] pt-2 text-[#064E3B]/70 leading-relaxed">
            {description}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button
            variant="outline"
            className="border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5"
            onClick={() => onOpenChange(false)}
          >
            {cancelLabel}
          </Button>
          <Button
            className={destructive ? 'bg-red-600 text-white hover:bg-red-700' : undefined}
            variant={destructive ? 'default' : 'accent'}
            onClick={() => {
              onConfirm();
              onOpenChange(false);
            }}
          >
            {confirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

const WEEK_MS = 7 * 24 * 60 * 60 * 1000;

function formatRelative(diffMs: number, date: Date): string {
  if (Math.abs(diffMs) >= WEEK_MS) {
    return new Intl.DateTimeFormat(undefined, {
      day: 'numeric',
      month: 'short',
      // Only show the year once it stops being the current one.
      year: date.getFullYear() === new Date().getFullYear() ? undefined : 'numeric',
    }).format(date);
  }

  const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' });
  const minutes = Math.round(diffMs / 60000);
  if (Math.abs(minutes) < 1) return 'just now';
  if (Math.abs(minutes) < 60) return rtf.format(-minutes, 'minute');
  const hours = Math.round(minutes / 60);
  if (Math.abs(hours) < 24) return rtf.format(-hours, 'hour');
  return rtf.format(-Math.round(hours / 24), 'day');
}
