import { AlertTriangle, Clock, Calendar } from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card';
import { cn } from '@/lib/utils';
import { useUpcomingBills } from '@/hooks/queries/useUpcomingBills';
import { classifyBillUrgency, type BillUrgency } from './classifyBillUrgency';

const URGENCY_STYLES: Record<BillUrgency, { badge: string; icon: typeof AlertTriangle; label: string }> = {
  overdue: { badge: 'bg-destructive/10 text-red-700', icon: AlertTriangle, label: 'Overdue' },
  critical: { badge: 'bg-destructive/10 text-red-700', icon: AlertTriangle, label: 'Due soon' },
  warning: { badge: 'bg-amber-500/10 text-amber-700', icon: Clock, label: 'Upcoming' },
  normal: { badge: 'bg-secondary text-muted-foreground', icon: Calendar, label: 'Scheduled' },
};

/** TASK-FE-008 (Doc 30): from `dashboard_upcoming_bills`, color-coded by urgency. */
export default function UpcomingBillsWidget() {
  const { data: bills, isLoading } = useUpcomingBills();

  return (
    <Card>
      <CardHeader>
        <CardTitle>Upcoming Bills</CardTitle>
        <CardDescription>Statement due dates across your instruments.</CardDescription>
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <p className="text-sm text-muted-foreground" role="status">Loading…</p>
        ) : !bills || bills.length === 0 ? (
          <p className="text-sm text-muted-foreground" role="status">No upcoming bills.</p>
        ) : (
          <ul className="space-y-2" aria-label="Upcoming bills">
            {bills.map((bill) => {
              const urgency = classifyBillUrgency(bill.due_date);
              const { badge, icon: Icon, label } = URGENCY_STYLES[urgency];
              return (
                <li key={bill.id} className="flex items-center justify-between p-2 rounded-md border border-border">
                  <div className="flex items-center gap-2 min-w-0">
                    <div className={cn('p-1.5 rounded-md shrink-0', badge)} aria-hidden="true">
                      <Icon className="w-3.5 h-3.5" />
                    </div>
                    <div className="min-w-0">
                      <p className="text-sm font-medium truncate">{bill.description}</p>
                      <p className="text-xs text-muted-foreground">
                        {label} — {new Date(bill.due_date).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })}
                      </p>
                    </div>
                  </div>
                  <span className="text-sm font-medium shrink-0 ml-2">₹ {bill.amount.toLocaleString()}</span>
                </li>
              );
            })}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}
