import { useState, useEffect } from 'react';
import { parseISODate } from '../dateHelpers';

/**
 * The month currently on screen and everything derived from it: the grid
 * geometry, the min/max bounds, and the year-jumper range.
 */
export function useCalendar(value: string | undefined, min?: string, max?: string) {
  const selectedDate = parseISODate(value);
  const minDate = parseISODate(min);
  const maxDate = parseISODate(max);
  const today = new Date();

  const [viewDate, setViewDate] = useState<Date>(() => selectedDate || today);

  // Sync viewDate when the value changes. `selectedDate` is re-derived here
  // rather than closed over: it is a fresh Date object on every render, so
  // depending on it directly would re-run this effect forever.
  useEffect(() => {
    const next = parseISODate(value);
    if (next) setViewDate(next);
  }, [value]);

  const year = viewDate.getFullYear();
  const month = viewDate.getMonth();

  const isToday = (dayNum: number) =>
    today.getFullYear() === year && today.getMonth() === month && today.getDate() === dayNum;

  const isSelected = (dayNum: number) =>
    !!selectedDate &&
    selectedDate.getFullYear() === year &&
    selectedDate.getMonth() === month &&
    selectedDate.getDate() === dayNum;

  /** Bounds compare on calendar days, so the min/max day itself stays usable. */
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

function startOfDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate());
}
