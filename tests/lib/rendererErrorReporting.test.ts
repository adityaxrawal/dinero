import { describe, it, expect, vi, beforeEach } from 'vitest';
import { API } from '@/lib/ipc';
import { reportRendererError, installGlobalErrorHandlers } from '@/lib/rendererErrorReporting';

vi.mock('@/lib/ipc', async () => {
  const actual = await vi.importActual<typeof import('@/lib/ipc')>('@/lib/ipc');
  return {
    ...actual,
    API: {
      support: {
        logRendererError: vi.fn().mockResolvedValue(undefined),
      },
    },
  };
});

describe('test_renderer_and_rust_errors_are_captured', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('reportRendererError forwards message/stack/source to the log_renderer_error IPC command', () => {
    reportRendererError('boom', 'at foo.tsx:1:1', 'react_error_boundary');
    expect(API.support.logRendererError).toHaveBeenCalledWith(
      'boom',
      'at foo.tsx:1:1',
      'react_error_boundary'
    );
  });

  it('never throws even if the IPC call itself rejects', async () => {
    vi.mocked(API.support.logRendererError).mockRejectedValueOnce(new Error('ipc down'));
    expect(() => reportRendererError('boom', undefined, 'window_onerror')).not.toThrow();
    await Promise.resolve();
  });

  it('installGlobalErrorHandlers captures an uncaught window error', () => {
    installGlobalErrorHandlers();
    const error = new Error('uncaught');
    window.dispatchEvent(new ErrorEvent('error', { message: 'uncaught', error }));
    expect(API.support.logRendererError).toHaveBeenCalledWith(
      'uncaught',
      error.stack,
      'window_onerror'
    );
  });

  it('installGlobalErrorHandlers captures an unhandled promise rejection', () => {
    installGlobalErrorHandlers();
    const reason = new Error('rejected');
    const event = new Event('unhandledrejection') as PromiseRejectionEvent & { reason: unknown };
    Object.defineProperty(event, 'reason', { value: reason });
    window.dispatchEvent(event);
    expect(API.support.logRendererError).toHaveBeenCalledWith(
      'rejected',
      reason.stack,
      'unhandled_rejection'
    );
  });
});
