/**
 * Visualises progress through an instalment plan.
 *
 * Turns what would otherwise look like unexplained repeating charges into a
 * single purchase with a visible schedule.
 */
import { CheckCircle2, Circle, Loader2 } from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card';
import { useEmiGroup } from '@/hooks/queries/useEmiGroup';

interface EmiInstallmentTimelineProps {
  emiGroupId: string;
}

/** Visualises progress through an instalment plan. */
export default function EmiInstallmentTimeline({ emiGroupId }: EmiInstallmentTimelineProps) {
  const { data: summary, isLoading } = useEmiGroup(emiGroupId);

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-6" role="status">
        <Loader2 className="w-4 h-4 animate-spin text-muted-foreground" aria-hidden="true" />
      </div>
    );
  }

  if (!summary) return null;

  const total = summary.total_installments ?? summary.installments_paid;
  const remaining = Math.max(0, total - summary.installments_paid);
  const pct = total > 0 ? Math.round((summary.installments_paid / total) * 100) : 0;

  return (
    <Card>
      <CardHeader>
        <CardTitle>EMI Installments</CardTitle>
        <CardDescription>
          {summary.installments_paid} of {total} paid
          {summary.total_installments === null &&
            ' (total unknown — no installment reported "X of Y" yet)'}
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div
          className="w-full h-2 bg-secondary rounded-full overflow-hidden"
          role="progressbar"
          aria-valuenow={pct}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-label="EMI installments paid"
        >
          <div
            className="h-full bg-[#064E3B] transition-all duration-300"
            style={{ width: `${pct}%` }}
          />
        </div>

        <ul className="space-y-1.5" aria-label="Installment list">
          {summary.installments.map((inst, i) => (
            <li key={inst.transaction_id} className="flex items-center gap-2 text-sm">
              <CheckCircle2 className="w-3.5 h-3.5 text-emerald-600 shrink-0" aria-hidden="true" />
              <span className="text-muted-foreground">Installment {i + 1}</span>
              <span className="flex-1" />
              <span className="text-xs text-muted-foreground">
                {inst.event_time ? new Date(inst.event_time).toLocaleDateString() : '—'}
              </span>
              <span className="font-medium">₹ {(inst.amount_minor / 100).toLocaleString()}</span>
            </li>
          ))}
          {Array.from({ length: remaining }).map((_, i) => (
            <li
              key={`remaining-${i}`}
              className="flex items-center gap-2 text-sm text-muted-foreground/60"
            >
              <Circle className="w-3.5 h-3.5 shrink-0" aria-hidden="true" />
              <span>Installment {summary.installments.length + i + 1} — upcoming</span>
            </li>
          ))}
        </ul>
      </CardContent>
    </Card>
  );
}
