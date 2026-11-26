/**
 * The instrument a transaction is attributed to.
 */
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { InstrumentSelect } from '@/components/instruments/InstrumentSelect';
import { TransactionAmountBalance } from '@/components/transactions/TransactionAmountBalance';
import type { useTransactionForm } from '@/components/transactions/useTransactionForm';

type Form = ReturnType<typeof useTransactionForm>;

/** The instrument this transaction is attributed to. */
export default function InstrumentCard({ form, tx }: { form: Form; tx: NonNullable<Form['tx']> }) {
  return (
    <Card className="bg-champagne/60 backdrop-blur-sm border-emerald-ink/10 shadow-xs">
      <CardHeader className="py-3.5 px-5 border-b border-emerald-ink/10 bg-emerald-ink/[0.03]">
        <CardTitle className="text-[12px] font-bold uppercase tracking-wider text-emerald-ink">
          Payment Instrument &amp; Balance
        </CardTitle>
      </CardHeader>
      <CardContent className="p-0">
        <InstrumentSelect
          instrumentId={form.instrumentId}
          onInstrumentChange={form.setInstrumentId}
          instruments={form.instruments}
        />
        {(tx.balance_after_transaction !== null || form.isForeignCurrency) && (
          <TransactionAmountBalance tx={tx} isForeignCurrency={form.isForeignCurrency} />
        )}
      </CardContent>
    </Card>
  );
}
