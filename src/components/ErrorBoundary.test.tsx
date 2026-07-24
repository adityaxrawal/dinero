import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { ErrorBoundary } from './ErrorBoundary';
import { reportRendererError } from '@/lib/rendererErrorReporting';

vi.mock('@/lib/rendererErrorReporting', () => ({
  reportRendererError: vi.fn(),
}));

vi.mock('@/lib/ipc', () => ({
  API: { support: { exportLogs: vi.fn() } },
}));

function Bomb(): never {
  throw new Error('render exploded');
}

describe('test_renderer_and_rust_errors_are_captured (React error boundary half)', () => {
  it('forwards a caught render exception to reportRendererError', () => {
    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    render(
      <ErrorBoundary>
        <Bomb />
      </ErrorBoundary>
    );
    expect(reportRendererError).toHaveBeenCalledWith(
      'render exploded',
      expect.any(String),
      'react_error_boundary'
    );
    expect(screen.getByRole('alert')).toBeInTheDocument();
    consoleErrorSpy.mockRestore();
  });
});
