import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import { Toaster } from './toaster';

let toasts: Array<Record<string, unknown>> = [];

vi.mock('@/hooks/use-toast', () => ({ useToast: () => ({ toasts }) }));
vi.mock('react-router-dom', () => ({
  Link: ({ to, children }: { to: string; children: React.ReactNode }) => (
    <a href={to}>{children}</a>
  ),
}));

beforeEach(() => {
  toasts = [];
});

describe('Toaster', () => {
  it('renders nothing when there are no toasts', () => {
    render(<Toaster />);
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('renders a title and description', () => {
    toasts = [{ id: '1', title: 'Saved', description: 'Your changes were stored.', open: true }];
    render(<Toaster />);
    expect(screen.getByText('Saved')).toBeInTheDocument();
    expect(screen.getByText('Your changes were stored.')).toBeInTheDocument();
  });

  it('omits the description when there is none', () => {
    toasts = [{ id: '1', title: 'Saved', open: true }];
    render(<Toaster />);
    expect(screen.getByText('Saved')).toBeInTheDocument();
  });

  it('renders a route action as a link', () => {
    toasts = [{ id: '1', title: 'Locked', actionTo: '/settings', actionLabel: 'Open Settings', open: true }];
    render(<Toaster />);
    const link = screen.getByText('Open Settings').closest('a');
    expect(link).toHaveAttribute('href', '/settings');
  });

  it('falls back to a custom action element when no route is given', () => {
    toasts = [{ id: '1', title: 'Undo?', action: <button>Undo</button>, open: true }];
    render(<Toaster />);
    expect(screen.getByText('Undo')).toBeInTheDocument();
  });

  it('prefers the route action when both a route and a custom action exist', () => {
    toasts = [
      {
        id: '1',
        title: 'Both',
        actionTo: '/settings',
        actionLabel: 'Open Settings',
        action: <button>Custom</button>,
        open: true,
      },
    ];
    render(<Toaster />);
    expect(screen.getByText('Open Settings')).toBeInTheDocument();
    expect(screen.queryByText('Custom')).toBeNull();
  });

  it('needs both actionTo and actionLabel to render a link', () => {
    toasts = [{ id: '1', title: 'Partial', actionTo: '/settings', open: true }];
    render(<Toaster />);
    expect(screen.queryByRole('link')).toBeNull();
  });

  it('renders every queued toast', () => {
    toasts = [
      { id: '1', title: 'First', open: true },
      { id: '2', title: 'Second', open: true },
    ];
    render(<Toaster />);
    expect(screen.getByText('First')).toBeInTheDocument();
    expect(screen.getByText('Second')).toBeInTheDocument();
  });
});
