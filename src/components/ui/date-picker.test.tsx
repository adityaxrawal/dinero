import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { DatePicker, DateRangePicker, parseISODate, toISODate, formatDisplayDate } from './date-picker';

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
