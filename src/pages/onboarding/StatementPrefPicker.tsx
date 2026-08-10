/**
 * Statement-handling preference picker.
 */
import { Mail, ShieldCheck, Check } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { Label } from '@/components/ui/label';
import type { StatementPref } from './useOnboardingPreferences';

const BASE = [
  'relative flex flex-col items-center p-5 rounded-xl border-[1.5px] text-sm transition-all duration-200 ease-out outline-none',
  'focus-visible:ring-2 focus-visible:ring-[#064E3B]/60 focus-visible:ring-offset-2 focus-visible:ring-offset-background',
  'hover:-translate-y-0.5',
].join(' ');

const SELECTED = [
  'border-[#064E3B]/70 font-semibold',
  'bg-[#064E3B]/8',
  'shadow-[0_0_0_1px_rgba(37,99,235,0.2)]',
  'text-[#053d2f]',
].join(' ');

const UNSELECTED =
  'border-border bg-background text-foreground hover:border-[#064E3B]/35 hover:bg-[#064E3B]/[0.04]';

/** One selectable statement-handling preference. */
function PrefOption({
  icon: Icon,
  title,
  caption,
  selected,
  onSelect,
}: {
  icon: LucideIcon;
  title: string;
  caption: string;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      onClick={onSelect}
      className={[BASE, selected ? SELECTED : UNSELECTED].join(' ')}
    >
      <span
        aria-hidden="true"
        className={[
          'absolute top-2 right-2 w-5 h-5 rounded-full flex items-center justify-center',
          'bg-[#064E3B]',
          'transition-all duration-200 ease-out',
          selected ? 'opacity-100 scale-100' : 'opacity-0 scale-0',
        ].join(' ')}
      >
        <Check className="w-3 h-3 text-white" strokeWidth={3} />
      </span>
      <Icon
        className={[
          'w-6 h-6 mb-2 transition-colors duration-200',
          selected ? 'text-[#064E3B]' : 'text-muted-foreground',
        ].join(' ')}
        aria-hidden="true"
      />
      <span className="font-medium">{title}</span>
      <span
        className={[
          'text-xs mt-0.5 transition-colors',
          selected ? 'text-[#053d2f]' : 'text-muted-foreground',
        ].join(' ')}
      >
        {caption}
      </span>
    </button>
  );
}

/** Statement-handling preference picker. */
export default function StatementPrefPicker({
  value,
  onChange,
}: {
  value: StatementPref;
  onChange: (pref: StatementPref) => void;
}) {
  return (
    <div className="space-y-2">
      <Label>Statement Preference</Label>
      <div className="grid grid-cols-2 gap-3" role="radiogroup" aria-label="Statement preference">
        <PrefOption
          icon={Mail}
          title="Auto (Gmail)"
          caption="Fetched from email"
          selected={value === 'auto'}
          onSelect={() => onChange('auto')}
        />
        <PrefOption
          icon={ShieldCheck}
          title="Manual"
          caption="Upload PDFs yourself"
          selected={value === 'manual'}
          onSelect={() => onChange('manual')}
        />
      </div>
    </div>
  );
}
