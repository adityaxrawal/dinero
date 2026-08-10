/**
 * Classifies unprocessed statements by what action they need.
 *
 * Separating "needs a password" from "failed to parse" matters because they call
 * for entirely different user actions.
 */
import { FileWarning, Lock, RotateCw } from 'lucide-react';
import type { UnprocessedStatementEntry } from '@/lib/ipc';

export type GroupKey = 'awaiting_password' | 'pending_retry' | 'failed';

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

/** Readable label for a queue entry. */
export function entryLabel(item: UnprocessedStatementEntry): string {
  return item.display_name || item.filename || 'Unknown file';
}
