/**
 * Last-resort capture for frontend errors that escaped every React boundary.
 *
 * React's own error boundaries only see failures thrown during render. Errors
 * raised from event handlers, timers, or rejected promises bypass them entirely
 * and would otherwise surface only in a webview console the user never opens.
 * The handlers installed here catch that remainder and forward it to the Rust
 * backend, so a packaged desktop build still produces a usable diagnostic trail.
 */
import { API } from '@/lib/ipc';
import { logger } from '@/lib/logger';

/**
 * Record one renderer error both locally and in the backend's log.
 *
 * `source` identifies which trap caught it, which is what distinguishes a
 * synchronous throw from an unhandled promise rejection when reading logs later.
 */
export function reportRendererError(
  message: string,
  stack: string | undefined,
  source: string
): void {
  logger.error(`Uncaught Renderer Error (${source}): ${message}`, { stack, source }, 'frontend');

  // The rejection is swallowed on purpose. This function runs while the app is
  // already in a failed state, and letting a failed *report* throw would either
  // trigger the unhandledrejection handler below and recurse, or mask the
  // original error with a less useful one.
  API.support.logRendererError(message, stack, source).catch(() => {
  });
}

/**
 * Attach the two global traps. Called once from the entry point, before React
 * mounts, so failures during the initial render are already covered.
 */
export function installGlobalErrorHandlers(): void {
  // Synchronous throws that reached the top of the stack.
  window.addEventListener('error', (event: ErrorEvent) => {
    reportRendererError(event.message, event.error?.stack, 'window_onerror');
  });

  // Rejected promises with no attached catch handler. The rejection reason is
  // not guaranteed to be an Error -- any value can be thrown -- so it is
  // narrowed before reading `.message`/`.stack`, with a string coercion as the
  // fallback for primitives.
  window.addEventListener('unhandledrejection', (event: PromiseRejectionEvent) => {
    const reason = event.reason;
    const message = reason instanceof Error ? reason.message : String(reason);
    const stack = reason instanceof Error ? reason.stack : undefined;
    reportRendererError(message, stack, 'unhandled_rejection');
  });
}
