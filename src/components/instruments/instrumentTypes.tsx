/**
 * Instrument type definitions and their icons.
 */
import { CreditCard, Landmark, Smartphone } from 'lucide-react';

export const INSTRUMENT_TYPES = [
  { value: 'credit_card', label: 'Credit Card' },
  { value: 'debit_card', label: 'Debit Card' },
  { value: 'bank_account', label: 'Bank Account' },
  { value: 'upi_vpa', label: 'UPI VPA' },
] as const;

/** Display label for an instrument type. */
export function instrumentTypeLabel(type: string): string {
  return INSTRUMENT_TYPES.find((t) => t.value === type)?.label ?? type;
}

/** Icon for an instrument type. */
export function instrumentIcon(type: string, size = 20) {
  switch (type) {
    case 'credit_card':
    case 'debit_card':
      return <CreditCard size={size} aria-hidden="true" />;
    case 'bank_account':
      return <Landmark size={size} aria-hidden="true" />;
    case 'upi_vpa':
      return <Smartphone size={size} aria-hidden="true" />;
    default:
      return <CreditCard size={size} aria-hidden="true" />;
  }
}
