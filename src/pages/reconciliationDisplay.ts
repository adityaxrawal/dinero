/**
 * Display helpers shared by the reconciliation screens.
 */
import { cleanTextForReader } from '@/components/common/gmailParsing';
import type { UnassignedTransactionRecord } from '@/lib/ipc';

const BANK_FINGERPRINTS: [pattern: RegExp, name: string][] = [
  [/HDFC/i, 'HDFC Bank'],
  [/IndusInd/i, 'IndusInd Bank'],
  [/ICICI/i, 'ICICI Bank'],
  [/Axis/i, 'Axis Bank'],
  [/SBI/i, 'SBI Bank'],
  [/Kotak/i, 'Kotak Bank'],
];

const ISSUE_LABELS: Record<string, string> = {
  extraction_failed: 'Missing Fields',
  issuer_name_not_found: 'Unknown Card/Bank',
};

/** Parses an alert payload, tolerating malformed JSON. */
export function readAlertPayload(rawJson: unknown, fallbackText: string) {
  const empty = { name: '', subject: '', html: '', text: fallbackText };
  if (!rawJson || typeof rawJson !== 'string') return empty;
  try {
    const parsed = JSON.parse(rawJson);
    return {
      name: parsed.sender || parsed.from || '',
      subject: parsed.subject || '',
      html: parsed.html || '',
      text: parsed.body || fallbackText,
    };
  } catch {
    return empty;
  }
}

/**
 * Removes the angle-bracketed address from a display name.
 *
 * Also strips surrounding quotes, which senders add around names containing
 * commas.
 */
export function stripAddressFromName(name: string): string {
  if (!name.includes('<')) return name;
  return name.split('<')[0].trim().replace(/^["']|["']$/g, '');
}

/**
 * Resolves a usable bank name, falling back to body fingerprints.
 *
 * Forwarded and relayed alerts frequently lose their original sender name, so a
 * generic placeholder is replaced by matching the body against known bank
 * fingerprints.
 */
export function resolveBankName(name: string, body: string): string {
  const isGeneric = !name || name === 'Bank Alert' || name === 'Bank / Service Alert';
  if (!isGeneric) return name;
  return BANK_FINGERPRINTS.find(([pattern]) => pattern.test(body))?.[1] || 'Bank Alert';
}

/** Formats minor units as rupees, or null when absent. */
export const formatRupees = (amountMinor: number | null | undefined): string | null =>
  amountMinor == null
    ? null
    : `₹${(amountMinor / 100).toLocaleString('en-IN', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;

/** Assembles the display fields for an unassigned transaction. */
export function getUnassignedDisplayInfo(item: UnassignedTransactionRecord) {
  const payload = readAlertPayload(item.raw_payload_json, item.body_snippet || '');
  const senderName = stripAddressFromName(payload.name || item.merchant_raw || '');

  const bodyText = cleanTextForReader(payload.html, payload.text);
  const name = resolveBankName(senderName, bodyText);

  const trimmedBody = bodyText.startsWith(name) ? bodyText.slice(name.length).trim() : bodyText;

  return {
    name,
    snippet: payload.subject || trimmedBody || 'Transaction alert details missing',
    amountStr: formatRupees(item.amount_minor),
    dateStr: item.event_time
      ? new Date(item.event_time).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })
      : '',
    issueLabel: ISSUE_LABELS[item.reason] || 'Action Needed',
    avatarLetter: name.charAt(0).toUpperCase(),
  };
}
