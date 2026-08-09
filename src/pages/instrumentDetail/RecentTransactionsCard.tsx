import { Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { cn } from '@/lib/utils';
import type { useInstrumentForm } from '@/components/instruments/useInstrumentForm';

type Form = ReturnType<typeof useInstrumentForm>;
type Transaction = Form['recentTransactions'][number];

function TransactionRow({ tx }: { tx: Transaction }) {
  const isDebit = tx.direction === 'debit';
  const amount = Math.abs(tx.amount).toLocaleString(undefined, { minimumFractionDigits: 2 });

  return (
    <li className="flex items-center justify-between text-sm py-1 border-b border-border/20 last:border-0">
      <div className="flex items-center gap-2">
        <span className="font-semibold text-[#064E3B]">{tx.merchant}</span>
        <span
          className={cn(
            'text-[9px] font-extrabold px-1.5 py-0.2 rounded uppercase tracking-wider border',
            isDebit
              ? 'bg-red-500/10 text-red-700 border-red-500/20'
              : 'bg-emerald-500/10 text-emerald-700 border-emerald-500/20'
          )}
        >
          {isDebit ? 'DEBIT' : 'CREDIT'}
        </span>
      </div>
      <span
        className={cn('font-bold font-mono text-xs', isDebit ? 'text-red-700' : 'text-emerald-700')}
      >
        {isDebit ? '−' : '+'}₹{amount}
      </span>
    </li>
  );
}

export default function RecentTransactionsCard({
  form,
  onViewAll,
}: {
  form: Form;
  onViewAll: () => void;
}) {
  const { recentTransactions, totalTxCount } = form;

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between space-y-0">
        <CardTitle>Recent Transactions</CardTitle>
        <Button variant="link" size="sm" onClick={onViewAll}>
          View All
        </Button>
      </CardHeader>
      <CardContent>
        {recentTransactions.length === 0 ? (
          <p className="text-sm text-muted-foreground">No transactions for this instrument yet.</p>
        ) : (
          <ul className="space-y-2">
            {recentTransactions.map((tx) => (
              <TransactionRow key={tx.id} tx={tx} />
            ))}
          </ul>
        )}

        {form.hasNextPage && (
          <div className="pt-3 text-center">
            <Button
              variant="outline"
              size="sm"
              onClick={() => form.fetchNextPage()}
              disabled={form.isFetchingNextPage}
              className="w-full text-xs font-bold"
            >
              {form.isFetchingNextPage ? (
                <>
                  <Loader2 className="w-3.5 h-3.5 mr-2 animate-spin" /> Loading transactions...
                </>
              ) : (
                `Load More Transactions (${recentTransactions.length} of ${totalTxCount})`
              )}
            </Button>
          </div>
        )}

        {recentTransactions.length > 0 && (
          <p className="text-center text-[11px] font-mono text-muted-foreground pt-2">
            Showing {recentTransactions.length} of {totalTxCount} transactions
          </p>
        )}
      </CardContent>
    </Card>
  );
}
