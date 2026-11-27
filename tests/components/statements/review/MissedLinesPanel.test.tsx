// Transaction-looking lines the parser didn't read (current-issues #4).
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import MissedLinesPanel from '@/components/statements/review/MissedLinesPanel';

const line = { page: 2, text: 'Ref 99 on 06/07/2026 paid SWIGGY 450.00', date: '2026-07-06', amount_minor: 45000 };

describe('MissedLinesPanel', () => {
  it('lists missed lines and adds one as a row', () => {
    const onAdd = vi.fn();
    render(<MissedLinesPanel lines={[line]} onAdd={onAdd} />);
    expect(screen.getByText(/1 line looks like a transaction/)).toBeInTheDocument();
    expect(screen.getByText(/SWIGGY/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Add as row' }));
    expect(onAdd).toHaveBeenCalledWith(line);
  });

  it('renders nothing when every line was read', () => {
    const { container } = render(<MissedLinesPanel lines={[]} onAdd={() => {}} />);
    expect(container).toBeEmptyDOMElement();
  });
});
