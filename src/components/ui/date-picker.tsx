import React, { useState, useRef, useEffect } from 'react';
import { Calendar as CalendarIcon, ChevronLeft, ChevronRight, X } from 'lucide-react';
import { cn } from '@/lib/utils';

// Helper to safely parse YYYY-MM-DD into a local Date without UTC shift
export function parseISODate(dateStr?: string | null): Date | null {
  if (!dateStr) return null;
  const parts = dateStr.slice(0, 10).split('-').map(Number);
  if (parts.length < 3 || parts.some(isNaN)) return null;
  const [year, month, day] = parts;
  return new Date(year, month - 1, day);
}

// Helper to format a Date into YYYY-MM-DD local string
export function toISODate(date: Date): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, '0');
  const d = String(date.getDate()).padStart(2, '0');
  return `${y}-${m}-${d}`;
}

// Helper for human display format (e.g. 26 Jul 2026)
export function formatDisplayDate(dateStr?: string | null): string {
  const parsed = parseISODate(dateStr);
  if (!parsed) return '';
  return parsed.toLocaleDateString('en-GB', {
    day: '2-digit',
    month: 'short',
    year: 'numeric',
  });
}

const MONTH_NAMES = [
  'January', 'February', 'March', 'April', 'May', 'June',
  'July', 'August', 'September', 'October', 'November', 'December'
];

const SHORT_DAYS = ['Su', 'Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa'];

export interface DatePickerProps {
  value?: string | null | undefined;
  onChange: (value: string) => void;
  min?: string | undefined;
  max?: string | undefined;
  placeholder?: string | undefined;
  disabled?: boolean | undefined;
  id?: string | undefined;
  className?: string | undefined;
  triggerClassName?: string | undefined;
  clearable?: boolean | undefined;
  size?: 'sm' | 'default' | undefined;
  'aria-label'?: string | undefined;
}

export const DatePicker: React.FC<DatePickerProps> = ({
  value,
  onChange,
  min,
  max,
  placeholder = 'Select date',
  disabled = false,
  id,
  className,
  triggerClassName,
  clearable = false,
  size = 'default',
  'aria-label': ariaLabel,
}) => {
  const [isOpen, setIsOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  const selectedDate = parseISODate(value);
  const minDate = parseISODate(min);
  const maxDate = parseISODate(max);

  const today = new Date();
  const [viewDate, setViewDate] = useState<Date>(() => selectedDate || today);

  // Sync viewDate when popover opens or value changes
  useEffect(() => {
    if (selectedDate) {
      setViewDate(selectedDate);
    }
  }, [value]);

  // Handle click outside to close popover
  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    }
    if (isOpen) {
      document.addEventListener('mousedown', handleClickOutside);
    }
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
    };
  }, [isOpen]);

  const year = viewDate.getFullYear();
  const month = viewDate.getMonth();

  // Generate calendar grid
  const firstDayOfMonth = new Date(year, month, 1).getDay();
  const daysInMonth = new Date(year, month + 1, 0).getDate();
  const daysInPrevMonth = new Date(year, month, 0).getDate();

  const handlePrevMonth = (e: React.MouseEvent) => {
    e.stopPropagation();
    setViewDate(new Date(year, month - 1, 1));
  };

  const handleNextMonth = (e: React.MouseEvent) => {
    e.stopPropagation();
    setViewDate(new Date(year, month + 1, 1));
  };

  const handleYearChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const newYear = parseInt(e.target.value, 10);
    setViewDate(new Date(newYear, month, 1));
  };

  const handleMonthChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const newMonth = parseInt(e.target.value, 10);
    setViewDate(new Date(year, newMonth, 1));
  };

  const handleSelectDay = (dayNum: number, _isCurrentMonth: boolean, offsetMonth = 0) => {
    const targetMonth = month + offsetMonth;
    const dateObj = new Date(year, targetMonth, dayNum);
    const iso = toISODate(dateObj);

    if (minDate && dateObj < minDate) return;
    if (maxDate && dateObj > maxDate) return;

    onChange(iso);
    setIsOpen(false);
  };

  const isToday = (dayNum: number) => {
    return (
      today.getFullYear() === year &&
      today.getMonth() === month &&
      today.getDate() === dayNum
    );
  };

  const isSelected = (dayNum: number) => {
    if (!selectedDate) return false;
    return (
      selectedDate.getFullYear() === year &&
      selectedDate.getMonth() === month &&
      selectedDate.getDate() === dayNum
    );
  };

  const isDateDisabled = (dayNum: number, offsetMonth = 0) => {
    const dateObj = new Date(year, month + offsetMonth, dayNum);
    if (minDate) {
      const minStart = new Date(minDate.getFullYear(), minDate.getMonth(), minDate.getDate());
      if (dateObj < minStart) return true;
    }
    if (maxDate) {
      const maxEnd = new Date(maxDate.getFullYear(), maxDate.getMonth(), maxDate.getDate());
      if (dateObj > maxEnd) return true;
    }
    return false;
  };

  // Year range options for fast jumper
  const minYear = minDate ? minDate.getFullYear() : Math.min(2010, year - 5);
  const maxYear = maxDate ? maxDate.getFullYear() : Math.max(today.getFullYear() + 5, year + 5);
  const years = Array.from({ length: maxYear - minYear + 1 }, (_, i) => minYear + i);

  return (
    <div ref={containerRef} className={cn('relative inline-block w-full', className)}>
      {/* Input Trigger Button */}
      <div
        id={id}
        tabIndex={disabled ? -1 : 0}
        role="button"
        aria-label={ariaLabel || placeholder}
        aria-expanded={isOpen}
        onClick={() => !disabled && setIsOpen(!isOpen)}
        onKeyDown={(e) => {
          if (!disabled && (e.key === 'Enter' || e.key === ' ')) {
            e.preventDefault();
            setIsOpen(!isOpen);
          }
        }}
        className={cn(
          'flex items-center justify-between gap-2 rounded-lg border transition-all cursor-pointer select-none outline-none',
          'bg-[#F8E7C9]/40 hover:bg-[#F8E7C9]/70 border-[#064E3B]/20 text-[#064E3B]',
          'focus-visible:ring-2 focus-visible:ring-[#064E3B] focus-visible:border-transparent',
          disabled && 'opacity-50 cursor-not-allowed pointer-events-none bg-black/5',
          isOpen && 'border-[#064E3B] ring-1 ring-[#064E3B] bg-[#F8E7C9]',
          size === 'sm' ? 'px-2.5 py-1 text-xs h-8' : 'px-3 py-2 text-xs md:text-sm h-10',
          triggerClassName
        )}
      >
        <div className="flex items-center gap-2 overflow-hidden truncate">
          <CalendarIcon className={cn('flex-shrink-0 text-[#064E3B]/70', size === 'sm' ? 'w-3.5 h-3.5' : 'w-4 h-4')} />
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
                onChange('');
              }}
              className="p-0.5 rounded-full hover:bg-[#064E3B]/10 text-[#064E3B]/60 hover:text-[#064E3B]"
              aria-label="Clear date"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          )}
        </div>
      </div>

      {/* Calendar Popover */}
      {isOpen && (
        <div className="absolute z-50 mt-1.5 w-72 rounded-xl bg-[#F3EBDD] border border-[#d9c8a8] shadow-xl p-3.5 animate-in fade-in slide-in-from-top-2 duration-150">
          {/* Header Controls (Month/Year dropdowns & Chevrons) */}
          <div className="flex items-center justify-between mb-3 gap-1">
            <button
              type="button"
              onClick={handlePrevMonth}
              className="p-1.5 rounded-md hover:bg-[#064E3B]/10 text-[#064E3B] transition-colors"
              aria-label="Previous month"
            >
              <ChevronLeft className="w-4 h-4" />
            </button>

            <div className="flex items-center gap-1.5">
              <select
                value={month}
                onChange={handleMonthChange}
                className="bg-[#F8E7C9] text-[#064E3B] font-semibold text-xs rounded-md px-1.5 py-1 border border-[#064E3B]/20 cursor-pointer outline-none focus:ring-1 focus:ring-[#064E3B]"
              >
                {MONTH_NAMES.map((name, idx) => (
                  <option key={name} value={idx}>
                    {name}
                  </option>
                ))}
              </select>

              <select
                value={year}
                onChange={handleYearChange}
                className="bg-[#F8E7C9] text-[#064E3B] font-semibold text-xs rounded-md px-1.5 py-1 border border-[#064E3B]/20 cursor-pointer outline-none focus:ring-1 focus:ring-[#064E3B]"
              >
                {years.map((y) => (
                  <option key={y} value={y}>
                    {y}
                  </option>
                ))}
              </select>
            </div>

            <button
              type="button"
              onClick={handleNextMonth}
              className="p-1.5 rounded-md hover:bg-[#064E3B]/10 text-[#064E3B] transition-colors"
              aria-label="Next month"
            >
              <ChevronRight className="w-4 h-4" />
            </button>
          </div>

          {/* Days of Week Header */}
          <div className="grid grid-cols-7 gap-1 text-center mb-1">
            {SHORT_DAYS.map((d) => (
              <span key={d} className="text-[11px] font-semibold text-[#064E3B]/60 py-1">
                {d}
              </span>
            ))}
          </div>

          {/* Calendar Day Cells */}
          <div className="grid grid-cols-7 gap-1 text-center">
            {/* Previous Month Trail */}
            {Array.from({ length: firstDayOfMonth }).map((_, i) => {
              const dayNum = daysInPrevMonth - firstDayOfMonth + i + 1;
              const disabled = isDateDisabled(dayNum, -1);
              return (
                <button
                  key={`prev-${i}`}
                  type="button"
                  disabled={disabled}
                  onClick={() => handleSelectDay(dayNum, false, -1)}
                  className={cn(
                    'h-8 w-8 rounded-lg text-xs flex items-center justify-center text-[#064E3B]/30 hover:bg-[#064E3B]/5 transition-colors mx-auto',
                    disabled && 'opacity-20 cursor-not-allowed pointer-events-none'
                  )}
                >
                  {dayNum}
                </button>
              );
            })}

            {/* Current Month Days */}
            {Array.from({ length: daysInMonth }).map((_, i) => {
              const dayNum = i + 1;
              const selected = isSelected(dayNum);
              const todayFlag = isToday(dayNum);
              const disabled = isDateDisabled(dayNum, 0);

              return (
                <button
                  key={`curr-${dayNum}`}
                  type="button"
                  disabled={disabled}
                  onClick={() => handleSelectDay(dayNum, true, 0)}
                  className={cn(
                    'h-8 w-8 rounded-lg text-xs font-medium flex items-center justify-center transition-all mx-auto relative',
                    selected
                      ? 'bg-[#064E3B] text-[#F8E7C9] font-bold shadow-md scale-105'
                      : 'hover:bg-[#064E3B]/10 text-[#064E3B]',
                    todayFlag && !selected && 'border border-[#064E3B] text-[#064E3B] font-bold bg-[#F8E7C9]/60',
                    disabled && 'opacity-30 cursor-not-allowed pointer-events-none'
                  )}
                >
                  {dayNum}
                  {todayFlag && !selected && (
                    <span className="absolute bottom-1 w-1 h-1 rounded-full bg-[#064E3B]" />
                  )}
                </button>
              );
            })}
          </div>

          {/* Bottom Quick Jump Action */}
          <div className="mt-3 pt-2.5 border-t border-[#d9c8a8]/60 flex items-center justify-between text-xs">
            <button
              type="button"
              onClick={() => {
                const todayISO = toISODate(new Date());
                onChange(todayISO);
                setViewDate(new Date());
                setIsOpen(false);
              }}
              className="text-[#064E3B] font-semibold hover:underline"
            >
              Today
            </button>
            {value && (
              <button
                type="button"
                onClick={() => {
                  onChange('');
                  setIsOpen(false);
                }}
                className="text-red-700 hover:underline"
              >
                Clear
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  );
};

/* ── Date Range Picker Component with Quick Presets ── */

export interface DateRangePickerProps {
  startDate?: string | undefined;
  endDate?: string | undefined;
  onChange: (range: { startDate: string; endDate: string }) => void;
  min?: string | undefined;
  max?: string | undefined;
  disabled?: boolean | undefined;
  className?: string | undefined;
  showPresets?: boolean | undefined;
}

export const DateRangePicker: React.FC<DateRangePickerProps> = ({
  startDate = '',
  endDate = '',
  onChange,
  min,
  max,
  disabled = false,
  className,
  showPresets = true,
}) => {
  const handlePreset = (daysBack: number | 'thisMonth' | 'lastMonth' | 'thisYear' | 'lastYear' | 'all') => {
    const end = new Date();
    let start = new Date();

    if (typeof daysBack === 'number') {
      start.setDate(end.getDate() - daysBack);
    } else if (daysBack === 'thisMonth') {
      start = new Date(end.getFullYear(), end.getMonth(), 1);
    } else if (daysBack === 'lastMonth') {
      start = new Date(end.getFullYear(), end.getMonth() - 1, 1);
      end.setDate(0); // last day of previous month
    } else if (daysBack === 'thisYear') {
      start = new Date(end.getFullYear(), 0, 1);
    } else if (daysBack === 'lastYear') {
      start = new Date(end.getFullYear() - 1, 0, 1);
      end.setFullYear(end.getFullYear() - 1, 11, 31);
    } else if (daysBack === 'all') {
      start = min ? (parseISODate(min) || new Date(2020, 0, 1)) : new Date(2020, 0, 1);
    }

    onChange({
      startDate: toISODate(start),
      endDate: toISODate(end),
    });
  };

  return (
    <div className={cn('space-y-3', className)}>
      {/* Quick Presets Bar */}
      {showPresets && (
        <div className="flex flex-wrap gap-1.5 text-xs">
          {[
            { label: '30 Days', action: () => handlePreset(30) },
            { label: '90 Days', action: () => handlePreset(90) },
            { label: 'This Month', action: () => handlePreset('thisMonth') },
            { label: 'This Year', action: () => handlePreset('thisYear') },
            { label: '1 Year', action: () => handlePreset(365) },
            { label: 'All Time', action: () => handlePreset('all') },
          ].map((preset) => (
            <button
              key={preset.label}
              type="button"
              disabled={disabled}
              onClick={preset.action}
              className="px-2.5 py-1 rounded-md text-[12px] font-medium bg-[#064E3B]/5 hover:bg-[#064E3B]/15 text-[#064E3B] border border-[#064E3B]/10 transition-colors disabled:opacity-50 cursor-pointer"
            >
              {preset.label}
            </button>
          ))}
        </div>
      )}

      {/* Start and End Date Inputs */}
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        <div className="space-y-1">
          <label className="text-[11px] font-semibold uppercase tracking-wider text-[#064E3B]/70">
            Start Date
          </label>
          <DatePicker
            value={startDate}
            onChange={(val) => onChange({ startDate: val, endDate })}
            min={min}
            max={endDate || max}
            disabled={disabled}
            placeholder="Select start date"
          />
        </div>

        <div className="space-y-1">
          <label className="text-[11px] font-semibold uppercase tracking-wider text-[#064E3B]/70">
            End Date
          </label>
          <DatePicker
            value={endDate}
            onChange={(val) => onChange({ startDate, endDate: val })}
            min={startDate || min}
            max={max}
            disabled={disabled}
            placeholder="Select end date"
          />
        </div>
      </div>
    </div>
  );
};
