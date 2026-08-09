import React from 'react';
import { cn } from '@/lib/utils';
import { parseISODate, toISODate } from './dateHelpers';
import { usePopover } from './datePicker/usePopover';
import { useCalendar } from './datePicker/useCalendar';
import CalendarPopover from './datePicker/CalendarPopover';
import DatePickerTrigger from './datePicker/DatePickerTrigger';

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
  const { isOpen, openUpward, containerRef, triggerRef, toggle, close } = usePopover(disabled);
  const calendar = useCalendar(value ?? undefined, min, max);

  const handleSelectDay = (dayNum: number, offsetMonth: number) => {
    const dateObj = new Date(calendar.year, calendar.month + offsetMonth, dayNum);
    if (calendar.minDate && dateObj < calendar.minDate) return;
    if (calendar.maxDate && dateObj > calendar.maxDate) return;

    onChange(toISODate(dateObj));
    close();
  };

  return (
    <div ref={containerRef} className={cn('relative inline-block w-full', className)}>
      <DatePickerTrigger
        triggerRef={triggerRef}
        id={id}
        value={value}
        placeholder={placeholder}
        ariaLabel={ariaLabel}
        disabled={disabled}
        isOpen={isOpen}
        size={size}
        clearable={clearable}
        triggerClassName={triggerClassName}
        onToggle={toggle}
        onClear={() => onChange('')}
      />

      {isOpen && (
        <CalendarPopover
          calendar={calendar}
          openUpward={openUpward}
          value={value}
          onChange={onChange}
          onSelectDay={handleSelectDay}
          onClose={close}
        />
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
