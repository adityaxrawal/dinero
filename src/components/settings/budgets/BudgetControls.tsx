/**
 * Threshold toggles and per-category budget cards for the budgets screen.
 */
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { cn } from '@/lib/utils';
import type { CategoryBudget } from '@/lib/ipc';

/** Toggle for one alert threshold level. */
export function ThresholdToggle({
  label,
  description,
  isActive,
  onToggle,
}: {
  label: string;
  description: string;
  isActive: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onToggle}
      className={cn(
        'relative flex flex-col items-center px-6 py-5 rounded-xl text-[13px] transition-all duration-200 outline-none',
        'min-w-[120px] border',
        isActive
          ? 'border-emerald-ink bg-emerald-ink/5 ring-1 ring-emerald-ink/20'
          : 'border-emerald-ink/20 bg-champagne/50 hover:border-emerald-ink/30 hover:bg-emerald-ink/5'
      )}
    >
      <span
        className={cn(
          'text-2xl font-bold transition-colors',
          isActive ? 'text-emerald-ink' : 'text-emerald-ink/70'
        )}
      >
        {label}
      </span>
      <span className="text-[12px] font-medium mt-1 text-emerald-ink/70">{description}</span>
      <span
        className={cn(
          'mt-3 px-2.5 py-0.5 rounded-full text-[10px] font-bold uppercase tracking-wider',
          isActive
            ? 'bg-emerald-ink/10 text-emerald-ink'
            : 'bg-emerald-ink/5 text-emerald-ink/60 border border-emerald-ink/10'
        )}
      >
        {isActive ? 'ON' : 'OFF'}
      </span>
    </button>
  );
}

/** Editable budget for a single category. */
export function CategoryBudgetCard({
  cat,
  onChange,
}: {
  cat: CategoryBudget;
  onChange: (name: string, value: string) => void;
}) {
  return (
    <div className="space-y-3 p-5 rounded-xl border border-emerald-ink/10 bg-emerald-ink/5">
      <Label htmlFor={`cat-${cat.name}`} className="text-[14px] font-bold text-emerald-ink">
        {cat.name}
      </Label>
      <div className="flex items-center gap-2">
        <span className="text-[13px] font-medium text-emerald-ink/70 shrink-0">₹</span>
        <Input
          id={`cat-${cat.name}`}
          type="number"
          min="0"
          value={cat.budget === 0 ? '' : cat.budget}
          placeholder="No limit"
          onChange={(e) => onChange(cat.name, e.target.value)}
          className="bg-champagne/50 border-emerald-ink/20 text-emerald-ink focus-visible:ring-emerald-ink"
        />
      </div>
      {cat.budget > 0 && (
        <p className="text-[12px] font-semibold text-emerald-ink/60">
          ₹ {cat.budget.toLocaleString()} / month
        </p>
      )}
    </div>
  );
}
