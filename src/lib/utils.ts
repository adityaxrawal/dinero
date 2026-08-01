import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

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

/** Display label for `Transaction.channel` (see `detect_channel` in the Rust extraction ladder). */
export function channelLabel(channel: string): string {
  return (
    CHANNEL_ACRONYMS[channel] ??
    channel
      .split('_')
      .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
      .join(' ')
  );
}

export function formatRelativeDate(dateString: string): string {
  const d = new Date(dateString);
  const today = new Date();
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  const isToday = d.toDateString() === today.toDateString();
  const isYesterday = d.toDateString() === yesterday.toDateString();
  return isToday
    ? 'Today'
    : isYesterday
      ? 'Yesterday'
      : d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

/** Used to flag stale queue items (e.g. reconciliation clusters pending review too long). */
export function isOlderThanDays(dateString: string, days: number): boolean {
  return Date.now() - new Date(dateString).getTime() > days * 24 * 60 * 60 * 1000;
}
