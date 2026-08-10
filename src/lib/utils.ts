/**
 * Small shared helpers used across the UI layer: class-name composition, error
 * coercion, and a few display formatters.
 *
 * Everything here is pure and dependency-light by design -- these are called
 * from render paths throughout the component tree, so nothing in this file
 * should reach for IPC, state, or the clock beyond `Date.now()`.
 */
import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

/**
 * Compose Tailwind class names, resolving conflicts in favour of the last one.
 *
 * The two libraries do different jobs and both are needed: clsx flattens
 * conditional and array inputs into a string, then tailwind-merge removes
 * earlier utilities that the later ones override. Without the merge step a
 * caller passing `px-2` to a component whose base styles already set `px-4`
 * would emit both, and the winner would depend on stylesheet order rather than
 * on the caller's intent.
 */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/**
 * Extract a readable message from a value thrown by anything.
 *
 * JavaScript permits throwing any value, so this narrows in three stages: a
 * real Error, then an object merely shaped like one (which is what a structured
 * IPC rejection looks like), then a plain string coercion as the final fallback.
 */
export function errorMessage(err: unknown): string {
  if (err instanceof Error) return err.message;
  if (typeof err === 'object' && err !== null && 'message' in err) {
    return String((err as { message: unknown }).message);
  }
  return String(err);
}

// Payment channels whose display form is an acronym or otherwise cannot be
// derived by title-casing the stored identifier. Anything absent from this map
// is formatted mechanically by channelLabel below.
const CHANNEL_ACRONYMS: Record<string, string> = {
  upi: 'UPI',
  upi_credit_card: 'UPI on Credit Card',
  imps: 'IMPS',
  neft: 'NEFT',
  rtgs: 'RTGS',
  pos: 'POS',
  atm: 'ATM',
  ecs_nach: 'ECS/NACH',
  bnpl: 'BNPL',
};

/**
 * Turn a stored payment-channel identifier into display text.
 *
 * Consults the acronym table first; otherwise splits the snake_case identifier
 * and title-cases each word, so a new channel added by the backend still renders
 * acceptably without a frontend change.
 */
export function channelLabel(channel: string): string {
  return (
    CHANNEL_ACRONYMS[channel] ??
    channel
      .split('_')
      .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
      .join(' ')
  );
}

/**
 * Render a date as "Today", "Yesterday", or an abbreviated month and day.
 *
 * Used in transaction lists, where the two most recent days carry the most
 * meaning and benefit from being named rather than dated.
 */
export function formatRelativeDate(dateString: string): string {
  const d = new Date(dateString);
  const today = new Date();
  const yesterday = new Date(today);

  // setDate handles month and year rollover, so this stays correct on the 1st.
  yesterday.setDate(today.getDate() - 1);

  // Comparison is on the calendar date only. toDateString discards the time
  // component, so a timestamp from earlier today still matches "Today" rather
  // than being judged by elapsed hours.
  const isToday = d.toDateString() === today.toDateString();
  const isYesterday = d.toDateString() === yesterday.toDateString();
  return isToday
    ? 'Today'
    : isYesterday
      ? 'Yesterday'
      : d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

/**
 * Whether a timestamp is further in the past than the given number of days.
 *
 * Compares elapsed milliseconds rather than calendar days, so "1 day" means a
 * full 24 hours and not "yesterday". Drives staleness prompts, such as flagging
 * a cluster that has sat unreviewed too long.
 */
export function isOlderThanDays(dateString: string, days: number): boolean {
  return Date.now() - new Date(dateString).getTime() > days * 24 * 60 * 60 * 1000;
}
