import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { InstrumentSelect } from '@/components/instruments/InstrumentSelect';
import { TransactionAmountBalance } from '@/components/transactions/TransactionAmountBalance';
import type { useTransactionForm } from '@/components/transactions/useTransactionForm';

type Form = ReturnType<typeof useTransactionForm>;

export default function InstrumentCard({
  form,
  tx,
}: {
  form: Form;
  tx: NonNullable<Form['tx']>;
}) {
  return (
    <Card className="bg-[#F8E7C9]/60 backdrop-blur-sm border-[#064E3B]/10 shadow-xs">
      <CardHeader className="py-3.5 px-5 border-b border-[#064E3B]/10 bg-[#064E3B]/[0.03]">
        <CardTitle className="text-[12px] font-bold uppercase tracking-wider text-[#064E3B]">
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
