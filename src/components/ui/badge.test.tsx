import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { Badge } from './badge';

describe('Badge', () => {
  it('renders its children as text content', () => {
    render(<Badge>ACTIVE</Badge>);
    expect(screen.getByText('ACTIVE')).toBeInTheDocument();
  });

  it('applies the destructive variant classes used for WCAG-compliant contrast', () => {
    render(<Badge variant="destructive">LOCKED</Badge>);
    const badge = screen.getByText('LOCKED');
    // Doc 14 §10: the darker red (red-700), not the raw --destructive token,
    // is required here to clear the 4.5:1 contrast bar for small text.
    expect(badge.className).toContain('bg-red-700');
  });

  it('merges a caller-supplied className without dropping variant classes', () => {
    render(<Badge className="custom-class">TAG</Badge>);
    const badge = screen.getByText('TAG');
    expect(badge.className).toContain('custom-class');
    expect(badge.className).toContain('rounded-md');
  });
});
