import { instrumentTypeLabel } from './instrumentTypes';

/**
 * UPI handle suffix → the bank behind it. A VPA carries no issuer field, so
 * the handle is the only signal for what to call it. Scanned in order, so a
 * more specific suffix must precede any shorter one it contains.
 */
const UPI_HANDLE_BANKS: [suffix: string, label: string][] = [
  ['@jupiter', 'Jupiter UPI'],
  ['@okicici', 'ICICI UPI'],
  ['@icici', 'ICICI UPI'],
  ['@okaxis', 'Axis UPI'],
  ['@axis', 'Axis UPI'],
  ['@oksbi', 'SBI UPI'],
  ['@sbi', 'SBI UPI'],
  ['@paytm', 'Paytm UPI'],
  ['@hdfc', 'HDFC UPI'],
];

function upiHandleLabel(maskedIdentifier: string): string {
  const handle = maskedIdentifier.toLowerCase();
  return UPI_HANDLE_BANKS.find(([suffix]) => handle.includes(suffix))?.[1] ?? 'UPI Payment Handle';
}

export function getInstrumentTitle(inst?: {
  issuer_name?: string | null;
  instrument_type: string;
  masked_identifier?: string | null;
}): string {
  if (!inst) return 'Select Instrument';
  if (inst.issuer_name?.trim()) return inst.issuer_name;
  if (inst.instrument_type === 'upi_vpa' && inst.masked_identifier) {
    return upiHandleLabel(inst.masked_identifier);
  }
  return instrumentTypeLabel(inst.instrument_type);
}

export function getInstrumentSubtitle(inst?: {
  instrument_type: string;
  masked_identifier?: string | null;
}): string {
  if (!inst) return 'Click to assign';
  const typeLabel = instrumentTypeLabel(inst.instrument_type);
  if (inst.masked_identifier) {
    return `${typeLabel} · ${inst.masked_identifier}`;
  }
  return typeLabel;
}
