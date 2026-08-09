import { cn } from '@/lib/utils';
import type { LicenseStatusResponse } from '@/lib/ipc';

function Cell({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <p className="text-[11px] font-bold uppercase tracking-wider mb-1 text-[#064E3B]/60">
        {label}
      </p>
      <p className="text-[15px] font-bold">{children}</p>
    </div>
  );
}

function stateTone(status: LicenseStatusResponse): string {
  if (status.state === 'LOCKED') return 'text-red-600';
  return status.is_active ? 'text-emerald-600' : 'text-[#064E3B]';
}

export default function LicenseStatusGrid({ status }: { status: LicenseStatusResponse }) {
  const isTrial = status.state === 'TRIAL' || status.state === 'TRIALING';
  const showRemaining = status.days_remaining != null && status.state !== 'GRACE';

  return (
    <div className="grid grid-cols-2 sm:grid-cols-4 gap-6 p-6 rounded-xl border border-[#064E3B]/10 bg-[#F8E7C9]/50">
      <div>
        <p className="text-[11px] font-bold uppercase tracking-wider mb-1 text-[#064E3B]/60">
          Status
        </p>
        <p className={cn('text-[15px] font-bold', stateTone(status))}>{status.state || 'Unknown'}</p>
      </div>

      {status.plan_id && (
        <Cell label="Plan">
          {status.plan_id} {status.billing_interval ? `(${status.billing_interval})` : ''}
        </Cell>
      )}

      {showRemaining && (
        <Cell label={isTrial ? 'Trial Left' : 'Remaining'}>{status.days_remaining} Days</Cell>
      )}

      {status.expiry_date && (
        <Cell label="Renews">{new Date(status.expiry_date).toLocaleDateString()}</Cell>
      )}
    </div>
  );
}
