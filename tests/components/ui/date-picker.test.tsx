import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { DatePicker, DateRangePicker } from '@/components/ui/date-picker';
import { parseISODate, toISODate, formatDisplayDate } from '@/components/ui/dateHelpers';

describe('date-picker utilities', () => {
  it('parses ISO date string correctly without timezone shift', () => {
    const d = parseISODate('2026-07-26');
    expect(d).not.toBeNull();
    expect(d?.getFullYear()).toBe(2026);
    expect(d?.getMonth()).toBe(6); // July is 0-indexed month 6
    expect(d?.getDate()).toBe(26);
  });

  it('formats Date object to ISO string correctly', () => {
    const d = new Date(2026, 6, 26);
    expect(toISODate(d)).toBe('2026-07-26');
  });

  it('formats display date string nicely', () => {
    expect(formatDisplayDate('2026-07-26')).toContain('2026');
    expect(formatDisplayDate('2026-07-26')).toContain('Jul');
    expect(formatDisplayDate(null)).toBe('');
  });
});

describe('DatePicker Component', () => {
  it('renders placeholder when value is empty', () => {
    render(<DatePicker value="" onChange={() => {}} placeholder="Select a date" />);
    expect(screen.getByText('Select a date')).toBeInTheDocument();
  });

  it('renders formatted date when value is provided', () => {
    render(<DatePicker value="2026-07-26" onChange={() => {}} />);
    expect(screen.getByText(/26 Jul 2026|Jul 26, 2026|26\/07\/2026/i)).toBeInTheDocument();
  });

  it('opens calendar popover on click', () => {
    render(<DatePicker value="2026-07-26" onChange={() => {}} />);
    const trigger = screen.getByRole('button');
    fireEvent.click(trigger);
    expect(screen.getByText('July')).toBeInTheDocument();
    expect(screen.getByText('Today')).toBeInTheDocument();
  });
});

describe('DateRangePicker Component', () => {
  it('renders start and end date pickers and presets', () => {
    render(
      <DateRangePicker
        startDate="2026-06-01"
        endDate="2026-07-26"
        onChange={() => {}}
      />
    );
    expect(screen.getByText('30 Days')).toBeInTheDocument();
    expect(screen.getByText('90 Days')).toBeInTheDocument();
    expect(screen.getByText('This Month')).toBeInTheDocument();
  });

  it('triggers onChange when preset button is clicked', () => {
    const handleChange = vi.fn();
    render(
      <DateRangePicker
        startDate="2026-06-01"
        endDate="2026-07-26"
        onChange={handleChange}
      />
    );
    fireEvent.click(screen.getByText('30 Days'));
    expect(handleChange).toHaveBeenCalled();
  });
});

describe('DateRangePicker presets', () => {
  const onChange = vi.fn();
  const renderRange = (props = {}) => {
    onChange.mockClear();
    render(<DateRangePicker startDate="" endDate="" onChange={onChange} {...props} />);
  };
  const lastRange = () => onChange.mock.calls[onChange.mock.calls.length - 1][0];

  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 6, 15)); // 15 Jul 2026
  });
  afterEach(() => vi.useRealTimers());

  it('counts a fixed number of days back from today', () => {
    renderRange();
    fireEvent.click(screen.getByText('30 Days'));
    expect(lastRange()).toEqual({ startDate: '2026-06-15', endDate: '2026-07-15' });
  });

  it('spans a full year back for the 1 Year preset', () => {
    renderRange();
    fireEvent.click(screen.getByText('1 Year'));
    expect(lastRange().startDate).toBe('2025-07-15');
  });

  it('starts This Month on the first of the month', () => {
    renderRange();
    fireEvent.click(screen.getByText('This Month'));
    expect(lastRange()).toEqual({ startDate: '2026-07-01', endDate: '2026-07-15' });
  });

  it('starts This Year on 1 January', () => {
    renderRange();
    fireEvent.click(screen.getByText('This Year'));
    expect(lastRange()).toEqual({ startDate: '2026-01-01', endDate: '2026-07-15' });
  });

  it('anchors All Time to the min bound when one is given', () => {
    renderRange({ min: '2023-03-01' });
    fireEvent.click(screen.getByText('All Time'));
    expect(lastRange().startDate).toBe('2023-03-01');
  });

  it('falls back to 2020 for All Time with no min bound', () => {
    renderRange();
    fireEvent.click(screen.getByText('All Time'));
    expect(lastRange().startDate).toBe('2020-01-01');
  });
});

describe('DatePicker day selection', () => {
  const openCalendar = (props = {}) => {
    const onChange = vi.fn();
    render(<DatePicker value="2026-07-15" onChange={onChange} {...props} />);
    fireEvent.click(screen.getByRole('button'));
    return onChange;
  };

  it('reports the picked day as an ISO date', () => {
    const onChange = openCalendar();
    fireEvent.click(screen.getByText('20'));
    expect(onChange).toHaveBeenCalledWith('2026-07-20');
  });

  it('closes the calendar once a day is picked', () => {
    openCalendar();
    fireEvent.click(screen.getByText('20'));
    expect(screen.queryByText('20')).toBeNull();
  });

  it('refuses a day before the min bound', () => {
    const onChange = openCalendar({ min: '2026-07-10' });
    fireEvent.click(screen.getByText('5'));
    expect(onChange).not.toHaveBeenCalled();
  });

  it('refuses a day after the max bound', () => {
    const onChange = openCalendar({ max: '2026-07-20' });
    fireEvent.click(screen.getByText('25'));
    expect(onChange).not.toHaveBeenCalled();
  });

  it('accepts a day inside the bounds', () => {
    const onChange = openCalendar({ min: '2026-07-10', max: '2026-07-20' });
    fireEvent.click(screen.getByText('15'));
    expect(onChange).toHaveBeenCalledWith('2026-07-15');
  });
});
