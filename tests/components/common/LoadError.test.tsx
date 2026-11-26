// The one "couldn't load" state (Phase 2 2.3).
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import LoadError from '@/components/common/LoadError';

describe('LoadError', () => {
  it('names what failed, says nothing is lost, and retries', () => {
    const onRetry = vi.fn();
    render(<LoadError what="your transactions" onRetry={onRetry} />);
    expect(screen.getByRole('alert')).toHaveTextContent("Couldn't load your transactions");
    expect(screen.getByText(/Your data is safe/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it('shows no button without a retry', () => {
    render(<LoadError what="x" />);
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });
});
