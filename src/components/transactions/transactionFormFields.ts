/**
 * Field definitions for the transaction form.
 */
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

/**
 * The transaction's amount in major units.
 *
 * Prefers the stored float where present and otherwise derives it from minor
 * units, which is the authoritative field.
 */
export function transactionAmount(tx: CanonicalTransaction): number {
  return tx.amount ?? (tx.amount_minor ?? 0) / 100;
}

/**
 * Projects a transaction into editable form fields.
 *
 * The amount is taken as an absolute value because direction is edited as its
 * own field -- a negative here would double-apply the sign on save.
 */
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

/**
 * Whether the transaction was billed in another currency.
 *
 * Requires the original currency to differ from the settled one, so a foreign
 * charge already in the home currency is not flagged.
 */
export function isForeignCurrencyTransaction(tx: CanonicalTransaction | undefined): boolean {
  return Boolean(
    !!tx?.original_amount_minor && tx.original_currency && tx.original_currency !== tx.currency
  );
}
