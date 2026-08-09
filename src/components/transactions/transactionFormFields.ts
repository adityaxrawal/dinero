import type { CanonicalTransaction } from '@/lib/ipc';

export interface TransactionFields {
  merchant: string;
  categoryId: string;
  notes: string;
  amountStr: string;
  direction: 'debit' | 'credit';
  eventTime: string;
  instrumentId: string;
}

/** Major units, from whichever of the two amount columns the row carries. */
export function transactionAmount(tx: CanonicalTransaction): number {
  return tx.amount ?? (tx.amount_minor ?? 0) / 100;
}

/** The editable fields as they stand on the server, i.e. the "not dirty" baseline. */
export function fieldsFromTransaction(tx: CanonicalTransaction | undefined): TransactionFields {
  return {
    merchant: tx?.merchant_display_name ?? '',
    categoryId: tx?.category_id ?? '',
    notes: tx?.notes ?? '',
    amountStr: tx ? Math.abs(transactionAmount(tx)).toString() : '0',
    direction: tx?.direction === 'credit' ? 'credit' : 'debit',
    eventTime: tx?.best_event_time ?? '',
    instrumentId: tx?.instrument_id ?? '',
  };
}

/** True when the row records an amount in a currency other than the account's. */
export function isForeignCurrencyTransaction(tx: CanonicalTransaction | undefined): boolean {
  return Boolean(
    !!tx?.original_amount_minor && tx.original_currency && tx.original_currency !== tx.currency
  );
}
