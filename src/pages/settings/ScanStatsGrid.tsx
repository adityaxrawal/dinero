/**
 * Counters for what a scan has found so far.
 */
import { Link } from 'react-router-dom';
import { Mail, ScanLine, CheckCircle, FileText, Ban, AlertTriangle, Inbox } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { ScanProgressPayload } from '@/lib/ipc';

/** Formats a counter, defaulting to zero when absent. */
const count = (value?: number | null) => value ?? 0;

/** Counters for what a scan has found so far. */
export default function ScanStatsGrid({ progress }: { progress: ScanProgressPayload }) {
  const errors = count(progress.errors);
  const tiles = [
    { icon: Mail, label: 'Fetched:', value: count(progress.total) },
    { icon: ScanLine, label: 'Processed:', value: count(progress.processed) },
    {
      icon: CheckCircle,
      label: 'Txns:',
      value: count(progress.transactions_found),
      iconClass: 'text-emerald-600',
    },
    { icon: FileText, label: 'Statements:', value: count(progress.statements_found) },
    {
      icon: Inbox,
      label: 'Needs review:',
      value: count(progress.sent_to_review),
      to: '/reconciliation?section=unassigned',
    },
    { icon: Ban, label: 'Ignored:', value: count(progress.non_financial) },
    {
      icon: AlertTriangle,
      label: 'Errors:',
      value: errors,
      iconClass: errors > 0 ? 'text-red-600' : '',
      valueClass: errors > 0 ? 'text-red-600 font-bold' : '',
    },
  ];

  return (
    <div className="grid grid-cols-2 md:grid-cols-3 gap-4">
      {tiles.map(({ icon: Icon, label, value, iconClass, valueClass, to }) => {
        const body = (
          <>
            <Icon className={cn('w-4 h-4 text-[#064E3B]/50', iconClass)} />
            <span className="text-[#064E3B]/60 font-medium">{label}</span>
            <strong className={cn('font-bold', valueClass)}>{value}</strong>
          </>
        );
        // A non-zero review count links to the queue, so the user can act on it.
        return to && value > 0 ? (
          <Link key={label} to={to} className="flex items-center gap-2 text-[13px] underline">
            {body}
          </Link>
        ) : (
          <div key={label} className="flex items-center gap-2 text-[13px]">
            {body}
          </div>
        );
      })}
    </div>
  );
}
