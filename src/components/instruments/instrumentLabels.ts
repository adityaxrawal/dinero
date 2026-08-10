/**
 * Display labels for instrument types and networks.
 */
import { instrumentTypeLabel } from './instrumentTypes';

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

/** Display label for a UPI handle. */
function upiHandleLabel(maskedIdentifier: string): string {
  const handle = maskedIdentifier.toLowerCase();
  return UPI_HANDLE_BANKS.find(([suffix]) => handle.includes(suffix))?.[1] ?? 'UPI Payment Handle';
}

/** Primary label for an instrument: nickname, else issuer. */
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

/** Secondary label: type and masked identifier. */
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
