// Covers the statement-preference radio pair extracted out of Onboarding.tsx.
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import StatementPrefPicker from '@/pages/onboarding/StatementPrefPicker';

describe('StatementPrefPicker', () => {
  it('marks exactly one option as checked', () => {
    render(<StatementPrefPicker value="auto" onChange={vi.fn()} />);
    const [auto, manual] = screen.getAllByRole('radio');
    expect(auto).toHaveAttribute('aria-checked', 'true');
    expect(manual).toHaveAttribute('aria-checked', 'false');
  });

  it('reports the other option when it is picked', () => {
    const onChange = vi.fn();
    render(<StatementPrefPicker value="auto" onChange={onChange} />);
    fireEvent.click(screen.getByText('Manual'));
    expect(onChange).toHaveBeenCalledWith('manual');
  });

  it('moves the selection when the value changes', () => {
    const { rerender } = render(<StatementPrefPicker value="auto" onChange={vi.fn()} />);
    rerender(<StatementPrefPicker value="manual" onChange={vi.fn()} />);
    const [auto, manual] = screen.getAllByRole('radio');
    expect(auto).toHaveAttribute('aria-checked', 'false');
    expect(manual).toHaveAttribute('aria-checked', 'true');
  });

  it('explains what each option does', () => {
    render(<StatementPrefPicker value="auto" onChange={vi.fn()} />);
    expect(screen.getByText('Fetched from email')).toBeInTheDocument();
    expect(screen.getByText('Upload PDFs yourself')).toBeInTheDocument();
  });
});
