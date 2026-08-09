import { AlertTriangle } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { LicenseStatusResponse } from '@/lib/ipc';

function graceMessage(daysRemaining: number | null | undefined): string {
  const window =
    daysRemaining != null ? ` (${daysRemaining} day${daysRemaining === 1 ? '' : 's'} remaining)` : '';
  return `Your subscription is in its grace period${window} — refresh once your payment is resolved.`;
}

/** Only rendered for the two states that need a warning; returns null otherwise. */
export default function LicenseStateBanner({ status }: { status: LicenseStatusResponse }) {
  const isLocked = status.state === 'LOCKED';
  if (!isLocked && status.state !== 'GRACE') return null;

  return (
    <div
      className={cn(
        'p-5 rounded-xl border flex items-center gap-3 text-[14px] font-medium',
        isLocked
          ? 'bg-red-500/10 border-red-500/20 text-red-700'
          : 'bg-amber-500/10 border-amber-500/20 text-amber-700'
      )}
    >
      <AlertTriangle className="w-5 h-5 shrink-0" />
      <p>
        {isLocked
          ? 'Your license is locked. Paid features are unavailable until you refresh or reactivate.'
          : graceMessage(status.days_remaining)}
      </p>
    </div>
  );
}
