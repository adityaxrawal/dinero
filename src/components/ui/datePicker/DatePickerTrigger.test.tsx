// The date-picker trigger is the part every one of the 7 call sites renders,
// so its disabled/clearable/size permutations are worth pinning directly.
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import DatePickerTrigger from './DatePickerTrigger';

const base = {
  triggerRef: { current: null } as React.RefObject<HTMLDivElement | null>,
  id: 'dp',
  value: '2026-08-09',
  placeholder: 'Select date',
  ariaLabel: undefined,
  disabled: false,
  isOpen: false,
  size: 'default' as const,
  clearable: false,
  triggerClassName: undefined,
  onToggle: vi.fn(),
  onClear: vi.fn(),
};

describe('DatePickerTrigger', () => {
  it('shows the formatted date, and the placeholder when empty', () => {
    const { unmount } = render(<DatePickerTrigger {...base} />);
    expect(screen.queryByText('Select date')).not.toBeInTheDocument();
    unmount();

    render(<DatePickerTrigger {...base} value="" />);
    expect(screen.getByText('Select date')).toBeInTheDocument();
  });

  it('labels itself from the placeholder unless given an explicit label', () => {
    const { unmount } = render(<DatePickerTrigger {...base} />);
    expect(screen.getByRole('button')).toHaveAttribute('aria-label', 'Select date');
    unmount();

    render(<DatePickerTrigger {...base} ariaLabel="Due date" />);
    expect(screen.getByRole('button')).toHaveAttribute('aria-label', 'Due date');
  });

  it('opens on click and on Enter or Space', () => {
    const onToggle = vi.fn();
    render(<DatePickerTrigger {...base} onToggle={onToggle} />);
    const trigger = screen.getByRole('button');

    fireEvent.click(trigger);
    fireEvent.keyDown(trigger, { key: 'Enter' });
    fireEvent.keyDown(trigger, { key: ' ' });
    expect(onToggle).toHaveBeenCalledTimes(3);
  });

  it('ignores the keyboard entirely while disabled', () => {
    const onToggle = vi.fn();
    render(<DatePickerTrigger {...base} disabled onToggle={onToggle} />);
    const trigger = screen.getByRole('button');

    fireEvent.keyDown(trigger, { key: 'Enter' });
    expect(onToggle).not.toHaveBeenCalled();
    expect(trigger).toHaveAttribute('tabindex', '-1');
  });

  it('offers Clear only when clearable, filled and enabled', () => {
    const cases: [Partial<typeof base>, boolean][] = [
      [{ clearable: true }, true],
      [{ clearable: false }, false],
      [{ clearable: true, value: '' }, false],
      [{ clearable: true, disabled: true }, false],
    ];
    for (const [over, expected] of cases) {
      const { unmount } = render(<DatePickerTrigger {...base} {...over} />);
      expect(!!screen.queryByLabelText('Clear date')).toBe(expected);
      unmount();
    }
  });

  it('clears without also opening the calendar', () => {
    const onClear = vi.fn();
    const onToggle = vi.fn();
    render(<DatePickerTrigger {...base} clearable onClear={onClear} onToggle={onToggle} />);
    fireEvent.click(screen.getByLabelText('Clear date'));
    expect(onClear).toHaveBeenCalled();
    expect(onToggle).not.toHaveBeenCalled();
  });

  it('reports its open state to assistive tech', () => {
    render(<DatePickerTrigger {...base} isOpen />);
    expect(screen.getByRole('button')).toHaveAttribute('aria-expanded', 'true');
  });

  it('applies the compact height at size sm', () => {
    const { container } = render(<DatePickerTrigger {...base} size="sm" />);
    expect(container.querySelector('[role="button"]')?.className).toContain('h-8');
  });
});
