import React from 'react';
import { Landmark } from 'lucide-react';
import { InfoRow } from '@/components/ui/InfoRow';
import { formatMoney } from '@/lib/formatMoney';

export function TransactionAmountBalance({
  tx,
  isForeignCurrency,
}: {
  tx: any;
  isForeignCurrency: boolean;
}) {
  if (tx.balance_after_transaction === null && !isForeignCurrency) return null;

  return (
    <>
      <InfoRow label="Currency">{tx.currency ?? 'INR'}</InfoRow>
      {isForeignCurrency && (
        <>
          <InfoRow label="Original Amount">
            {formatMoney(tx.original_amount_minor, tx.original_currency)}
          </InfoRow>
          {tx.exchange_rate !== null && (
            <InfoRow label="Exchange Rate">{tx.exchange_rate?.toFixed(4)}</InfoRow>
          )}
        </>
      )}
      {tx.balance_after_transaction !== null && (
        <InfoRow icon={<Landmark className="w-3.5 h-3.5" />} label="Balance After">
          ₹
          {tx.balance_after_transaction?.toLocaleString(undefined, {
            minimumFractionDigits: 2,
          })}
        </InfoRow>
      )}
    </>
  );
}
