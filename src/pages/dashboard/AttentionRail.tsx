import { useNavigate } from 'react-router-dom';
import { AlertCircle, AlertTriangle, Calendar, ChevronRight, GitMerge } from 'lucide-react';
import { classifyBillUrgency } from '@/components/dashboard/classifyBillUrgency';
import type { useDashboardData } from './useDashboardData';

type Data = ReturnType<typeof useDashboardData>;

function AttentionCard({
  icon,
  iconBg,
  title,
  subtitle,
  ctaLabel,
  onClick,
  urgent,
}: {
  icon: React.ReactNode;
  iconBg: string;
  title: string;
  subtitle: string;
  ctaLabel: string;
  onClick: () => void;
  urgent?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="attention-card text-left focus-visible:outline-none"
      style={{
        borderColor: urgent ? 'rgba(245, 158, 11, 0.4)' : undefined,
        background: urgent ? 'rgba(245, 158, 11, 0.06)' : undefined,
      }}
    >
      <div
        className="flex items-center justify-center rounded-xl"
        style={{ width: 34, height: 34, backgroundColor: iconBg }}
        aria-hidden="true"
      >
        {icon}
      </div>
      <div className="min-w-0">
        <p className="text-sm font-semibold leading-tight" style={{ color: 'var(--text-primary)' }}>
          {title}
        </p>
        <p className="text-xs mt-0.5 leading-snug" style={{ color: 'var(--text-muted)' }}>
          {subtitle}
        </p>
      </div>
      <div
        className="flex items-center gap-1 text-xs font-medium mt-auto"
        style={{ color: '#064E3B' }}
      >
        {ctaLabel}
        <ChevronRight className="w-3 h-3" aria-hidden="true" />
      </div>
    </button>
  );
}

function BillCard({
  bill,
  onClick,
}: {
  bill: Data['urgentBills'][number];
  onClick: () => void;
}) {
  const urgency = classifyBillUrgency(bill.due_date);
  const overdue = urgency === 'overdue';
  const due = new Date(bill.due_date).toLocaleDateString(undefined, {
    month: 'short',
    day: 'numeric',
  });

  return (
    <AttentionCard
      icon={
        <AlertTriangle className="w-4 h-4" style={{ color: overdue ? '#ef4444' : '#f59e0b' }} />
      }
      iconBg={overdue ? 'rgba(239,68,68,0.12)' : 'rgba(245,158,11,0.12)'}
      title={bill.description}
      subtitle={`₹${bill.amount.toLocaleString()} · Due ${due}`}
      ctaLabel={overdue ? 'Overdue' : 'Due soon'}
      onClick={onClick}
      urgent={overdue}
    />
  );
}

export default function AttentionRail({ data }: { data: Data }) {
  const navigate = useNavigate();
  const { pending, urgentBills, clusters, summary } = data;
  const pendingCount = pending?.count ?? 0;
  const upcomingCount = summary?.upcoming_bills_count ?? 0;

  return (
    <section aria-label="Items needing attention" className="mb-5">
      <p className="section-heading" style={{ paddingLeft: 0 }}>
        Needs Attention
      </p>
      <div className="attention-rail mt-2">
        {pendingCount > 0 && pending && (
          <AttentionCard
            icon={<AlertCircle className="w-4 h-4" style={{ color: '#f59e0b' }} />}
            iconBg="rgba(245, 158, 11, 0.12)"
            title={`${pending.count} Pending Review`}
            subtitle={`₹${(pending.amount_minor / 100).toLocaleString(undefined, { minimumFractionDigits: 0 })} not yet confirmed`}
            ctaLabel="Review"
            onClick={() => navigate('/reconciliation')}
            urgent
          />
        )}

        {urgentBills.map((bill) => (
          <BillCard key={bill.id} bill={bill} onClick={() => navigate('/instruments')} />
        ))}

        {clusters.length > 0 && (
          <AttentionCard
            icon={<GitMerge className="w-4 h-4" style={{ color: '#064E3B' }} />}
            iconBg="rgba(6,78,59,0.10)"
            title={`${clusters.length} Unresolved Cluster${clusters.length > 1 ? 's' : ''}`}
            subtitle="Ambiguous transaction matches"
            ctaLabel="Resolve"
            onClick={() => navigate('/reconciliation')}
          />
        )}

        {upcomingCount > 0 && urgentBills.length === 0 && (
          <AttentionCard
            icon={<Calendar className="w-4 h-4" style={{ color: '#3d5a50' }} />}
            iconBg="rgba(6,78,59,0.08)"
            title={`${upcomingCount} Upcoming Bill${upcomingCount > 1 ? 's' : ''}`}
            subtitle="Statement due dates"
            ctaLabel="View"
            onClick={() => navigate('/instruments')}
          />
        )}
      </div>
    </section>
  );
}
