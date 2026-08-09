import { FileWarning, Lock, RotateCw } from 'lucide-react';
import type { UnprocessedStatementEntry } from '@/lib/ipc';

export type GroupKey = 'awaiting_password' | 'pending_retry' | 'failed';

/**
 * Issue #7: grouped by *what the user has to do about it*, not by the
 * backend's status enum. "pending_retry" and "failed" are two different
 * internal states that ask the same thing of a person — try it again — so
 * they sit together, while a locked PDF is the only group that genuinely
 * needs something the app cannot supply.
 */
export const GROUPS: {
  key: GroupKey;
  label: string;
  hint: string;
  icon: typeof Lock;
  action: string;
}[] = [
  {
    key: 'awaiting_password',
    label: 'Needs a password',
    hint: 'No stored password opens these. Add one and they parse automatically next time.',
    icon: Lock,
    action: 'Enter Password',
  },
  {
    key: 'pending_retry',
    label: 'Waiting to retry',
    hint: 'Interrupted part-way through. Re-parsing usually clears these.',
    icon: RotateCw,
    action: 'Retry',
  },
  {
    key: 'failed',
    label: "Couldn't be read",
    hint: 'The pipeline could not extract anything usable from these files.',
    icon: FileWarning,
    action: 'Retry',
  },
];

/**
 * Issue #9: the backend declines to invent a name when it cannot identify the
 * issuer, in which case the filename the bank itself chose is the more
 * informative label.
 */
export function entryLabel(item: UnprocessedStatementEntry): string {
  return item.display_name || item.filename || 'Unknown file';
}
