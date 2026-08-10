/**
 * Calendar grid state: visible month, day cells, and selection.
 *
 * Separated from rendering so the month arithmetic -- where off-by-one, leap
 * year and month-length errors hide -- is testable without a DOM.
 */
import { useState, useEffect } from 'react';
import { parseISODate } from '../dateHelpers';

/**
 * Calendar grid state: visible month, day cells, and selection.
 *
 * Kept apart from rendering so the month arithmetic -- where off-by-one, leap
 * year and month-length bugs hide -- is testable without a DOM.
 */
export function useCalendar(value: string | undefined, min?: string, max?: string) {
  const selectedDate = parseISODate(value);
  const minDate = parseISODate(min);
  const maxDate = parseISODate(max);
  const today = new Date();

  const [viewDate, setViewDate] = useState<Date>(() => selectedDate || today);

  useEffect(() => {
    const next = parseISODate(value);
    if (next) setViewDate(next);
  }, [value]);

  const year = viewDate.getFullYear();
  const month = viewDate.getMonth();

  /** Whether a day is today, compared by calendar date rather than timestamp. */
  const isToday = (dayNum: number) =>
    today.getFullYear() === year && today.getMonth() === month && today.getDate() === dayNum;

  /** Whether a day is the current selection. */
  const isSelected = (dayNum: number) =>
    !!selectedDate &&
    selectedDate.getFullYear() === year &&
    selectedDate.getMonth() === month &&
    selectedDate.getDate() === dayNum;

  /** Whether a day falls outside the allowed min/max range. */
  const isDateDisabled = (dayNum: number, offsetMonth = 0) => {
    const dateObj = new Date(year, month + offsetMonth, dayNum);
    if (minDate && dateObj < startOfDay(minDate)) return true;
    if (maxDate && dateObj > startOfDay(maxDate)) return true;
    return false;
  };

  const minYear = minDate ? minDate.getFullYear() : Math.min(2010, year - 5);
  const maxYear = maxDate ? maxDate.getFullYear() : Math.max(today.getFullYear() + 5, year + 5);

  return {
    viewDate,
    setViewDate,
    year,
    month,
    minDate,
    maxDate,
    firstDayOfMonth: new Date(year, month, 1).getDay(),
    daysInMonth: new Date(year, month + 1, 0).getDate(),
    daysInPrevMonth: new Date(year, month, 0).getDate(),
    years: Array.from({ length: maxYear - minYear + 1 }, (_, i) => minYear + i),
    isToday,
    isSelected,
    isDateDisabled,
    goToMonth: (delta: number) => setViewDate(new Date(year, month + delta, 1)),
    setYear: (newYear: number) => setViewDate(new Date(newYear, month, 1)),
    setMonth: (newMonth: number) => setViewDate(new Date(year, newMonth, 1)),
  };
}

/**
 * Midnight local time for a date.
 *
 * Range comparisons are at day granularity, so the time component is discarded --
 * otherwise a bound set earlier today would wrongly exclude today.
 */
function startOfDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate());
}
