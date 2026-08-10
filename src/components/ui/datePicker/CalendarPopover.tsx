/**
 * The month grid shown inside a date picker's popover.
 *
 * Presentational: it renders whatever cells `useCalendar` computed and reports
 * clicks back, so month arithmetic stays out of the render path.
 */
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { cn } from '@/lib/utils';
import { toISODate } from '../dateHelpers';
import type { useCalendar } from './useCalendar';

type Calendar = ReturnType<typeof useCalendar>;

const MONTH_NAMES = [
  'January', 'February', 'March', 'April', 'May', 'June',
  'July', 'August', 'September', 'October', 'November', 'December',
];

const SHORT_DAYS = ['Su', 'Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa'];

const SELECT_CLASS =
  'bg-[#F8E7C9] text-[#064E3B] font-semibold text-xs rounded-md px-1.5 py-1 border border-[#064E3B]/20 cursor-pointer outline-none focus:ring-1 focus:ring-[#064E3B]';
const NAV_CLASS = 'p-1.5 rounded-md hover:bg-[#064E3B]/10 text-[#064E3B] transition-colors';
const DAY_CLASS = 'h-8 w-8 rounded-lg text-xs flex items-center justify-center transition-all mx-auto';

/** Month title with previous/next navigation. */
function CalendarHeader({ calendar }: { calendar: Calendar }) {
  return (
    <div className="flex items-center justify-between mb-3 gap-1">
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          calendar.goToMonth(-1);
        }}
        className={NAV_CLASS}
        aria-label="Previous month"
      >
        <ChevronLeft className="w-4 h-4" />
      </button>

      <div className="flex items-center gap-1.5">
        <select
          value={calendar.month}
          onChange={(e) => calendar.setMonth(parseInt(e.target.value, 10))}
          className={SELECT_CLASS}
        >
          {MONTH_NAMES.map((name, idx) => (
            <option key={name} value={idx}>
              {name}
            </option>
          ))}
        </select>

        <select
          value={calendar.year}
          onChange={(e) => calendar.setYear(parseInt(e.target.value, 10))}
          className={SELECT_CLASS}
        >
          {calendar.years.map((y) => (
            <option key={y} value={y}>
              {y}
            </option>
          ))}
        </select>
      </div>

      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          calendar.goToMonth(1);
        }}
        className={NAV_CLASS}
        aria-label="Next month"
      >
        <ChevronRight className="w-4 h-4" />
      </button>
    </div>
  );
}

/** The day cells for the visible month. */
function DayGrid({
  calendar,
  onSelectDay,
}: {
  calendar: Calendar;
  onSelectDay: (dayNum: number, offsetMonth: number) => void;
}) {
  const { firstDayOfMonth, daysInMonth, daysInPrevMonth } = calendar;

  return (
    <div className="grid grid-cols-7 gap-1 text-center">
      {Array.from({ length: firstDayOfMonth }).map((_, i) => {
        const dayNum = daysInPrevMonth - firstDayOfMonth + i + 1;
        const disabled = calendar.isDateDisabled(dayNum, -1);
        return (
          <button
            key={`prev-${i}`}
            type="button"
            disabled={disabled}
            onClick={() => onSelectDay(dayNum, -1)}
            className={cn(
              DAY_CLASS,
              'text-[#064E3B]/30 hover:bg-[#064E3B]/5',
              disabled && 'opacity-20 cursor-not-allowed pointer-events-none'
            )}
          >
            {dayNum}
          </button>
        );
      })}

      {Array.from({ length: daysInMonth }).map((_, i) => {
        const dayNum = i + 1;
        const selected = calendar.isSelected(dayNum);
        const todayFlag = calendar.isToday(dayNum);
        const disabled = calendar.isDateDisabled(dayNum, 0);

        return (
          <button
            key={`curr-${dayNum}`}
            type="button"
            disabled={disabled}
            onClick={() => onSelectDay(dayNum, 0)}
            className={cn(
              DAY_CLASS,
              'font-medium relative',
              selected
                ? 'bg-[#064E3B] text-[#F8E7C9] font-bold shadow-md scale-105'
                : 'hover:bg-[#064E3B]/10 text-[#064E3B]',
              todayFlag &&
                !selected &&
                'border border-[#064E3B] text-[#064E3B] font-bold bg-[#F8E7C9]/60',
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
  );
}

/** Month grid shown inside the picker's popover. */
export default function CalendarPopover({
  calendar,
  openUpward,
  value,
  onChange,
  onSelectDay,
  onClose,
}: {
  calendar: Calendar;
  openUpward: boolean;
  value: string | null | undefined;
  onChange: (value: string) => void;
  onSelectDay: (dayNum: number, offsetMonth: number) => void;
  onClose: () => void;
}) {
  return (
    <div
      className={cn(
        'absolute z-50 w-72 rounded-xl bg-[#F3EBDD] border border-[#d9c8a8] shadow-xl p-3.5 duration-150',
        openUpward
          ? 'bottom-full mb-1.5 animate-in fade-in slide-in-from-bottom-2'
          : 'top-full mt-1.5 animate-in fade-in slide-in-from-top-2'
      )}
    >
      <CalendarHeader calendar={calendar} />

      <div className="grid grid-cols-7 gap-1 text-center mb-1">
        {SHORT_DAYS.map((d) => (
          <span key={d} className="text-[11px] font-semibold text-[#064E3B]/60 py-1">
            {d}
          </span>
        ))}
      </div>

      <DayGrid calendar={calendar} onSelectDay={onSelectDay} />

      <div className="mt-3 pt-2.5 border-t border-[#d9c8a8]/60 flex items-center justify-between text-xs">
        <button
          type="button"
          onClick={() => {
            onChange(toISODate(new Date()));
            calendar.setViewDate(new Date());
            onClose();
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
              onClose();
            }}
            className="text-red-700 hover:underline"
          >
            Clear
          </button>
        )}
      </div>
    </div>
  );
}
