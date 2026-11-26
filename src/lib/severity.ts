/**
 * The one set of severity levels every feedback channel uses (audit Phase 2
 * 2.1): the notification feed, toasts and banners. Before this the feed had
 * info/success/warning/error, system warnings critical/degraded/info, and
 * toasts default/destructive, with no shared meaning. Each surface keeps
 * its own colours (dark feed, light page) but keys them by these levels.
 *
 * - info: something happened, nothing to do.
 * - success: an action or background job finished as hoped.
 * - attention: the user should act; nothing is broken.
 * - error: something failed.
 */
export type Severity = 'info' | 'success' | 'attention' | 'error';

/** A backend system warning's level, in the shared set. */
export function fromSystemWarning(level: 'critical' | 'degraded' | 'info'): Severity {
  return level === 'critical' ? 'error' : level === 'degraded' ? 'attention' : 'info';
}
