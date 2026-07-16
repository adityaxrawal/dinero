import { useNavigate } from 'react-router-dom';
import { AlertCircle } from 'lucide-react';
import { usePendingReviewCount } from '@/hooks/queries/usePendingReviewCount';

/**
 * TASK-FE-008 (Doc 30): from `analytics_pending_review_count` — must
 * visually and semantically communicate this count is separate from
 * confirmed totals (SpendSummaryCard's numbers exclude it entirely; this
 * banner is the only place it's surfaced). Renders nothing when the count
 * is zero — an empty amber banner would just be noise.
 */
export default function PendingReviewBanner() {
  const navigate = useNavigate();
  const { data: pending } = usePendingReviewCount();

  if (!pending || pending.count === 0) return null;

  const amount = (pending.amount_minor / 100).toLocaleString(undefined, { minimumFractionDigits: 2 });

  return (
    <div
      role="status"
      className="flex items-center justify-between gap-3 bg-amber-500/10 border border-amber-500/30 rounded-lg px-4 py-3"
    >
      <div className="flex items-center gap-3">
        <AlertCircle className="w-4 h-4 text-amber-600 shrink-0" aria-hidden="true" />
        <p className="text-sm text-amber-900">
          <span className="font-medium">
            {pending.count} transaction{pending.count === 1 ? '' : 's'} (₹ {amount})
          </span>{' '}
          need your review — <span className="font-medium">not included in the totals above</span>.
        </p>
      </div>
      <button
        type="button"
        onClick={() => navigate('/reconciliation')}
        className="text-sm font-medium text-amber-900 underline underline-offset-2 hover:no-underline shrink-0"
      >
        Review now
      </button>
    </div>
  );
}
