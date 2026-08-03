/**
 * Display helpers for the reconciliation queue, extracted from
 * Reconciliation.tsx so they can be tested directly — same precedent as
 * gmailParsing.ts and evidenceDescription.ts. These decide what an
 * unassigned Gmail alert *looks* like in the list; none of them touch IPC
 * or React.
 */
import { cleanTextForReader } from '@/components/common/gmailParsing';
import type { UnassignedTransactionRecord } from '@/lib/ipc';

// Body-text fingerprint -> display name, checked in order.
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

/** The Gmail payload is an opaque JSON blob; pull out the fields we display. */
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

/** "IndusInd Bank <alerts@indusind.com>" -> "IndusInd Bank" */
export function stripAddressFromName(name: string): string {
  if (!name.includes('<')) return name;
  // Trim before unquoting: senders are usually written `"HDFC Bank" <a@b>`,
  // so the closing quote is not the last character until the space between
  // it and the angle bracket is gone. Stripping first left a stray `"`.
  return name.split('<')[0].trim().replace(/^["']|["']$/g, '');
}

/** Falls back to fingerprinting the body when the sender name says nothing. */
export function resolveBankName(name: string, body: string): string {
  const isGeneric = !name || name === 'Bank Alert' || name === 'Bank / Service Alert';
  if (!isGeneric) return name;
  return BANK_FINGERPRINTS.find(([pattern]) => pattern.test(body))?.[1] || 'Bank Alert';
}

// Note: en-IN grouping (₹1,00,000), which lib/formatMoney.ts does not apply.
export const formatRupees = (amountMinor: number | null | undefined): string | null =>
  amountMinor == null
    ? null
    : `₹${(amountMinor / 100).toLocaleString('en-IN', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;

export function getUnassignedDisplayInfo(item: UnassignedTransactionRecord) {
  const payload = readAlertPayload(item.raw_payload_json, item.body_snippet || '');
  const senderName = stripAddressFromName(payload.name || item.merchant_raw || '');

  const bodyText = cleanTextForReader(payload.html, payload.text);
  const name = resolveBankName(senderName, bodyText);

  // Snippets often repeat the bank name they open with — drop the echo.
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
