/**
 * Display helpers shared by the reconciliation screens.
 */
import { reviewReasonLabel } from '@/lib/outcomeVocabulary';
import { cleanTextForReader } from '@/components/common/gmailParsing';
import type { UnassignedTransactionRecord } from '@/lib/ipc';
import { formatMoney } from '@/lib/formatMoney';

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
  return name
    .split('<')[0]
    .trim()
    .replace(/^["']|["']$/g, '');
}

/** Assembles the display fields for an unassigned transaction. */
export function getUnassignedDisplayInfo(item: UnassignedTransactionRecord) {
  const payload = readAlertPayload(item.raw_payload_json, item.body_snippet || '');
  const senderName = stripAddressFromName(payload.name || item.merchant_raw || '');

  const bodyText = cleanTextForReader(payload.html, payload.text);
  // Only what the email itself says: its sender name, or the issuer it named.
  // The title used to be guessed from bank names found in the body (F9-11).
  const generic = !senderName || senderName === 'Bank Alert' || senderName === 'Bank / Service Alert';
  const name = generic ? item.rule_issuer || 'Bank alert' : senderName;

  const trimmedBody = bodyText.startsWith(name) ? bodyText.slice(name.length).trim() : bodyText;

  return {
    name,
    snippet: payload.subject || trimmedBody || 'Transaction alert details missing',
    // Signed, in the row's own currency (audit F9-8).
    amountStr:
      item.amount_minor == null ? null : formatMoney(item.amount_minor, item.currency, item.direction),
    dateStr: item.event_time
      ? new Date(item.event_time).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })
      : '',
    issueLabel: reviewReasonLabel(item.reason),
    avatarLetter: name.charAt(0).toUpperCase(),
  };
}
