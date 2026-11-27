/**
 * Lines on readable pages that look like transactions (a date and an
 * amount) but didn't become rows (current-issues #4). Each can be added as a
 * row, pre-filled from the line, to check against the PDF.
 */
import { Button } from '@/components/ui/button';
import { formatMoney } from '@/lib/formatMoney';
import type { MissedLine } from '@/lib/ipc';

export default function MissedLinesPanel({
  lines,
  onAdd,
}: {
  lines: MissedLine[];
  onAdd: (line: MissedLine) => void;
}) {
  if (lines.length === 0) return null;
  return (
    <section
      aria-label="Lines that weren't read"
      className="rounded-md border border-amber-300 bg-amber-50 p-3 text-[12px]"
    >
      <p className="font-medium text-amber-900">
        {lines.length === 1 ? '1 line looks' : `${lines.length} lines look`} like a transaction
        but wasn't read
      </p>
      <p className="mt-0.5 text-amber-900/85">Check each against the PDF and add it if it is one.</p>
      <ul className="mt-2 space-y-1.5 max-h-40 overflow-y-auto">
        {lines.map((line) => (
          <li key={`${line.page}:${line.text}`} className="flex items-center gap-2">
            <span className="flex-1 min-w-0 truncate font-mono" title={line.text}>
              p.{line.page} · {line.text}
            </span>
            {line.amount_minor != null && (
              <span className="shrink-0">{formatMoney(line.amount_minor, 'INR')}</span>
            )}
            <Button size="sm" variant="outline" className="h-6 shrink-0" onClick={() => onAdd(line)}>
              Add as row
            </Button>
          </li>
        ))}
      </ul>
    </section>
  );
}
