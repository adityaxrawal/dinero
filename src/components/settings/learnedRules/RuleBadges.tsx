/**
 * Status and field badges for a learned rule.
 *
 * Colour communicates whether a rule is live, retired or failing, so the health
 * of the rule set is readable at a glance.
 */
import { cn } from '@/lib/utils';
import { FIELD_CHIPS, STATUS_LABELS, STATUS_STYLES } from './labels';

/** Badge showing whether a rule is live, retired or failing. */
export function StatusBadge({ status }: { status: string }) {
  return (
    <span
      className={cn(
        'text-[10px] font-bold uppercase tracking-wide px-2 py-0.5 rounded-full border shrink-0',
        STATUS_STYLES[status] ?? STATUS_STYLES.inactive
      )}
    >
      {STATUS_LABELS[status] ?? status}
    </span>
  );
}

/** Chip naming the field a rule extracts. */
export function FieldChip({ field }: { field: string }) {
  return (
    <span className="text-[10px] font-semibold px-1.5 py-0.5 rounded bg-[#064E3B]/[0.07] text-[#064E3B]/75">
      {FIELD_CHIPS[field] ?? field}
    </span>
  );
}
