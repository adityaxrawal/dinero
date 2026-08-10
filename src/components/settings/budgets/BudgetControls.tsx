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
          ? 'border-[#064E3B] bg-[#064E3B]/5 ring-1 ring-[#064E3B]/20'
          : 'border-[#064E3B]/20 bg-[#F8E7C9]/50 hover:border-[#064E3B]/30 hover:bg-[#064E3B]/5'
      )}
    >
      <span
        className={cn(
          'text-2xl font-bold transition-colors',
          isActive ? 'text-[#064E3B]' : 'text-[#064E3B]/70'
        )}
      >
        {label}
      </span>
      <span className="text-[12px] font-medium mt-1 text-[#064E3B]/70">{description}</span>
      <span
        className={cn(
          'mt-3 px-2.5 py-0.5 rounded-full text-[10px] font-bold uppercase tracking-wider',
          isActive
            ? 'bg-[#064E3B]/10 text-[#064E3B]'
            : 'bg-[#064E3B]/5 text-[#064E3B]/60 border border-[#064E3B]/10'
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
    <div className="space-y-3 p-5 rounded-xl border border-[#064E3B]/10 bg-[#064E3B]/5">
      <Label htmlFor={`cat-${cat.name}`} className="text-[14px] font-bold text-[#064E3B]">
        {cat.name}
      </Label>
      <div className="flex items-center gap-2">
        <span className="text-[13px] font-medium text-[#064E3B]/70 shrink-0">₹</span>
        <Input
          id={`cat-${cat.name}`}
          type="number"
          min="0"
          value={cat.budget === 0 ? '' : cat.budget}
          placeholder="No limit"
          onChange={(e) => onChange(cat.name, e.target.value)}
          className="bg-[#F8E7C9]/50 border-[#064E3B]/20 text-[#064E3B] focus-visible:ring-[#064E3B]"
        />
      </div>
      {cat.budget > 0 && (
        <p className="text-[12px] font-semibold text-[#064E3B]/60">
          ₹ {cat.budget.toLocaleString()} / month
        </p>
      )}
    </div>
  );
}
