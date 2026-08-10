/**
 * The button that opens a date picker and displays the current selection.
 */
import { Calendar as CalendarIcon, X } from 'lucide-react';
import { cn } from '@/lib/utils';
import { formatDisplayDate } from '../dateHelpers';

const BASE_TRIGGER = [
  'flex items-center justify-between gap-2 rounded-lg border transition-all cursor-pointer select-none outline-none',
  'bg-[#F8E7C9]/40 hover:bg-[#F8E7C9]/70 border-[#064E3B]/20 text-[#064E3B]',
  'focus-visible:ring-2 focus-visible:ring-[#064E3B] focus-visible:border-transparent',
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
    isOpen && 'border-[#064E3B] ring-1 ring-[#064E3B] bg-[#F8E7C9]',
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
          className={cn('flex-shrink-0 text-[#064E3B]/70', size === 'sm' ? 'w-3.5 h-3.5' : 'w-4 h-4')}
        />
        <span className={cn('truncate font-medium', !value && 'text-[#064E3B]/50 font-normal')}>
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
            className="p-0.5 rounded-full hover:bg-[#064E3B]/10 text-[#064E3B]/60 hover:text-[#064E3B]"
            aria-label="Clear date"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        )}
      </div>
    </div>
  );
}
