import { API } from '@/lib/ipc';

/**
 * TASK-OPS-004: forwards a renderer-side error to Rust-side logging
 * (`log_renderer_error`) so it's captured in `app-logs.log` and included in
 * a diagnostic bundle export, the same as a Rust panic already is. Never
 * throws itself -- a reporting failure must not compound the error being
 * reported.
 */
export function reportRendererError(message: string, stack: string | undefined, source: string): void {
  API.support.logRendererError(message, stack, source).catch(() => {
    // Best-effort only. If IPC itself is broken, there is nothing further
    // to do here -- console.error (already called by every caller of this
    // function) remains the fallback visible in a dev session.
  });
}

/**
 * TASK-OPS-004 acceptance: `test_renderer_and_rust_errors_are_captured`.
 * Before this, only React render-time exceptions (caught by
 * `ErrorBoundary.componentDidCatch`) were captured at all, and even those
 * only reached `console.error` -- an uncaught exception outside a component
 * render (an event handler, a timer, a promise rejection) had no capture
 * path whatsoever. Call once at app startup (`main.tsx`).
 */
export function installGlobalErrorHandlers(): void {
  window.addEventListener('error', (event: ErrorEvent) => {
    reportRendererError(event.message, event.error?.stack, 'window_onerror');
  });

  window.addEventListener('unhandledrejection', (event: PromiseRejectionEvent) => {
    const reason = event.reason;
    const message = reason instanceof Error ? reason.message : String(reason);
    const stack = reason instanceof Error ? reason.stack : undefined;
    reportRendererError(message, stack, 'unhandled_rejection');
  });
}
