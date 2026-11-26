/**
 * The button that opens a date picker and displays the current selection.
 */
import { Calendar as CalendarIcon, X } from 'lucide-react';
import { cn } from '@/lib/utils';
import { formatDisplayDate } from '../dateHelpers';

const BASE_TRIGGER = [
  'flex items-center justify-between gap-2 rounded-lg border transition-all cursor-pointer select-none outline-none',
  'bg-champagne/40 hover:bg-champagne/70 border-emerald-ink/20 text-emerald-ink',
  'focus-visible:ring-2 focus-visible:ring-emerald-ink focus-visible:border-transparent',
].join(' ');

/** Trigger styling, varying with disabled and open state. */
function triggerClasses({
  disabled,
  isOpen,
  size,
  triggerClassName,
}: {
  disabled: boolean;
  isOpen: boolean;
  size: 'sm' | 'default';
  triggerClassName: string | undefined;
}) {
  return cn(
    BASE_TRIGGER,
    disabled && 'opacity-50 cursor-not-allowed pointer-events-none bg-black/5',
    isOpen && 'border-emerald-ink ring-1 ring-emerald-ink bg-champagne',
    size === 'sm' ? 'px-2.5 py-1 text-xs h-8' : 'px-3 py-2 text-xs md:text-sm h-10',
    triggerClassName
  );
}

/** Button opening the picker and showing the current selection. */
export default function DatePickerTrigger({
  triggerRef,
  id,
  value,
  placeholder,
  ariaLabel,
  disabled,
  isOpen,
  size,
  clearable,
  triggerClassName,
  onToggle,
  onClear,
}: {
  triggerRef: React.RefObject<HTMLDivElement | null>;
  id: string | undefined;
  value: string | null | undefined;
  placeholder: string;
  ariaLabel: string | undefined;
  disabled: boolean;
  isOpen: boolean;
  size: 'sm' | 'default';
  clearable: boolean;
  triggerClassName: string | undefined;
  onToggle: () => void;
  onClear: () => void;
}) {
  return (
    <div
      ref={triggerRef}
      id={id}
      tabIndex={disabled ? -1 : 0}
      role="button"
      aria-label={ariaLabel || placeholder}
      aria-expanded={isOpen}
      onClick={onToggle}
      onKeyDown={(e) => {
        if (!disabled && (e.key === 'Enter' || e.key === ' ')) {
          e.preventDefault();
          onToggle();
        }
      }}
      className={triggerClasses({ disabled, isOpen, size, triggerClassName })}
    >
      <div className="flex items-center gap-2 overflow-hidden truncate">
        <CalendarIcon
          className={cn(
            'flex-shrink-0 text-emerald-ink/85',
            size === 'sm' ? 'w-3.5 h-3.5' : 'w-4 h-4'
          )}
        />
        <span className={cn('truncate font-medium', !value && 'text-emerald-ink/80 font-normal')}>
          {value ? formatDisplayDate(value) : placeholder}
        </span>
      </div>

      <div className="flex items-center gap-1">
        {clearable && value && !disabled && (
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              onClear();
            }}
            className="p-0.5 rounded-full hover:bg-emerald-ink/10 text-emerald-ink/85 hover:text-emerald-ink"
            aria-label="Clear date"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        )}
      </div>
    </div>
  );
}
