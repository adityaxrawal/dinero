/**
 * Shared field styling for the inspector cards, so every card aligns identically.
 */
import type { LucideIcon } from 'lucide-react';
import { cn } from '@/lib/utils';
import { Label } from '@/components/ui/label';

export const FIELD_INPUT =
  'h-9 text-[13px] font-semibold bg-[#F3EBDD]/80 border-[#064E3B]/15 text-[#064E3B] focus-visible:ring-1 focus-visible:ring-[#064E3B]/30 rounded-xl';
export const FIELD_SELECT =
  'h-9 text-[13px] font-bold bg-[#F3EBDD]/80 border-[#064E3B]/15 text-[#064E3B] focus:ring-1 focus:ring-[#064E3B]/30 rounded-xl';
const FIELD_LABEL = 'text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70';

/** Card wrapper for a group of instrument fields. */
export function SpecCard({
  icon: Icon,
  title,
  hint,
  children,
  bodyClassName = 'space-y-3',
}: {
  icon: LucideIcon;
  title: string;
  hint: string;
  children: React.ReactNode;
  bodyClassName?: string;
}) {
  return (
    <div className="bg-[#F8E7C9]/60 rounded-2xl p-4 border border-[#064E3B]/10 space-y-3.5 shadow-xs">
      <h4 className="text-[11px] font-bold uppercase tracking-wider text-[#064E3B]/70 border-b border-[#064E3B]/10 pb-2 flex items-center justify-between">
        <span className="flex items-center gap-1.5">
          <Icon className="w-3.5 h-3.5 text-[#064E3B]" /> {title}
        </span>
        <span className="text-[10px] font-mono text-[#064E3B]/50">{hint}</span>
      </h4>
      <div className={bodyClassName}>{children}</div>
    </div>
  );
}

/** Label-and-input pair, aligned consistently across cards. */
export function LabeledField({
  htmlFor,
  label,
  labelClassName,
  children,
}: {
  htmlFor?: string;
  label: string;
  labelClassName?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-1">
      <Label htmlFor={htmlFor} className={cn(FIELD_LABEL, labelClassName)}>
        {label}
      </Label>
      {children}
    </div>
  );
}
