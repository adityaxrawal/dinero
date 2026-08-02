/**
 * Pure parsing helpers behind GmailEmailViewer: turning raw bank-alert HTML and
 * headers into readable text, quick-fill candidates and a sender identity.
 * Kept out of the component file so the component module only exports
 * components (react-refresh) and so these stay directly testable.
 */

export type QuickCandidate = { type: 'amount' | 'merchant' | 'date' | 'ref'; label: string; value: string };

/**
 * Strips raw CSS rules, style blocks, and non-content code from string.
 */
function sanitizeCssAndCode(input: string): string {
  if (!input) return '';
  
  let cleaned = input;
  // Remove full <style>...</style> blocks
  cleaned = cleaned.replace(/<style[\s\S]*?<\/style>/gi, ' ');
  // Remove <script>...</script> blocks
  cleaned = cleaned.replace(/<script[\s\S]*?<\/script>/gi, ' ');
  // Remove @media queries
  cleaned = cleaned.replace(/@media[^{]+\{(?:[^{}]*|\{[^{}]*\})*\}/gi, ' ');
  // Remove any CSS rule declaration blocks: e.g. body { ... }, body, table, td, a { ... }, .cls { ... }
  cleaned = cleaned.replace(/(?:^|\n)\s*(?:[a-z0-9_#.:\-\s,>+*()]+\s*\{[^}]*\})/gi, ' ');
  // Remove loose braces with CSS-like properties
  cleaned = cleaned.replace(/\{[^}]*(?:margin|padding|color|font-|border-|display:|text-align|width:|height:|background)[^}]*\}/gi, ' ');
  
  return cleaned;
}

/**
 * Converts raw HTML or text into clean, readable plain text for Reader View.
 */
export function cleanTextForReader(rawHtml?: string | null, rawText?: string | null): string {
  let textContent = '';

  if (rawText && rawText.trim()) {
    textContent = rawText;
  } else if (rawHtml && rawHtml.trim()) {
    // Strip HTML tags to get pure text content
    textContent = rawHtml.replace(/<br\s*\/?>/gi, '\n')
                         .replace(/<\/p>/gi, '\n\n')
                         .replace(/<\/tr>/gi, '\n')
                         .replace(/<\/td>/gi, '  ')
                         .replace(/<[^>]+>/g, '');
  }

  // Strip all residual CSS code
  textContent = sanitizeCssAndCode(textContent);

  // Normalize whitespace: max 2 consecutive newlines, collapse trailing spaces
  return textContent
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .join('\n\n');
}

/**
 * Prepares and repairs email HTML content so it renders accurately in Gmail iframe view.
 */
export function prepareGmailHtml(rawHtml?: string | null, rawText?: string | null): string {
  let content = rawHtml || '';

  // Fallback: If no HTML provided, check if rawText has HTML tags
  if (!content.trim() && rawText) {
    const isHtmlTag = /<(table|tr|td|th|div|p|span|b|strong|em|i|u|h[1-6]|ul|ol|li|br|hr|html|body|head|header|footer|section|article|a|img|pre|code)[^>]*>/i.test(rawText);
    if (isHtmlTag) {
      content = rawText;
    }
  }

  if (!content.trim()) {
    return '';
  }

  // Check if content has bare CSS rules without <style> tag
  if (!content.includes('<style')) {
    const hasHtmlTags = /<[a-z1-6][^>]*>/i.test(content);
    if (hasHtmlTags) {
      const cssMatch = content.match(/(?:@media[^{]+\{(?:[^{}]*|\{[^{}]*\})*\}|[a-z0-9_#.:\-\s,>+*()]+\s*\{[^}]*\}|\{[^}]*\})/gi);
      if (cssMatch && cssMatch.length > 0) {
        const cssBlock = cssMatch.join('\n');
        const htmlOnly = content.replace(/(?:@media[^{]+\{(?:[^{}]*|\{[^{}]*\})*\}|[a-z0-9_#.:\-\s,>+*()]+\s*\{[^}]*\}|\{[^}]*\})/gi, '').trim();
        content = `<style>\n${cssBlock}\n</style>\n${htmlOnly}`;
      }
    }
  }

  // Construct isolated HTML document for the iframe
  return `<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <base target="_blank">
  <style>
    /* Base Gmail iframe resets */
    html, body {
      margin: 0;
      padding: 16px;
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
      font-size: 13px;
      line-height: 1.5;
      color: #1e293b;
      background-color: #ffffff;
      word-wrap: break-word;
      overflow-wrap: break-word;
    }
    img {
      max-width: 100% !important;
      height: auto !important;
    }
    table {
      max-width: 100% !important;
    }
    a {
      color: #2563eb;
      text-decoration: underline;
    }
  </style>
</head>
<body>
  ${content}
</body>
</html>`;
}


/**
 * Runs one capture-group regex over `text`, de-duplicating on the normalised
 * capture. `build` turns a capture into a candidate, or returns null to reject
 * it. Rejected captures stay de-duplicated, which is harmless: the same text
 * would be rejected again.
 */
function scanCandidates(
  text: string,
  regex: RegExp,
  build: (captured: string) => QuickCandidate | null,
  normalize: (raw: string) => string = (raw) => raw
): QuickCandidate[] {
  const found: QuickCandidate[] = [];
  const seen = new Set<string>();
  let match: RegExpExecArray | null;
  while ((match = regex.exec(text)) !== null) {
    const captured = normalize(match[1]);
    if (seen.has(captured)) continue;
    seen.add(captured);
    const candidate = build(captured);
    if (candidate) found.push(candidate);
  }
  return found;
}

/**
 * Extracts candidate financial entities from text content for Quick-Fill Chips.
 */
export function extractQuickCandidates(text: string): QuickCandidate[] {
  if (!text) return [];

  // Amounts (e.g. INR 450.00, Rs. 1,200, ₹500.00)
  const amounts = scanCandidates(
    text,
    /(?:INR|RS\.?|₹)\s*([\d,]+(?:\.\d{1,2})?)/gi,
    (value) => {
      const num = parseFloat(value);
      if (isNaN(num) || num <= 0) return null;
      return { type: 'amount', label: `Amount: ₹${num.toLocaleString('en-IN')}`, value: num.toFixed(2) };
    },
    (raw) => raw.replace(/,/g, '')
  );

  // Dates (e.g. 26-Jul-2026, 2026-07-26, 26/07/2026)
  const dates = scanCandidates(
    text,
    /\b(\d{4}-\d{2}-\d{2}|\d{1,2}[-/\s](?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)[a-z]*[-/\s]\d{2,4})\b/gi,
    (value) => {
      const parsed = new Date(value);
      if (isNaN(parsed.getTime())) return null;
      const formatted = parsed.toISOString().slice(0, 10);
      return { type: 'date', label: `Date: ${formatted}`, value: formatted };
    }
  );

  // Reference / Txn IDs (e.g. Ref: 12345678, Txn ID: TXN987654)
  const refs = scanCandidates(
    text,
    /\b(?:ref|reference|txn|transaction|utr|rrn)[\s#:]*([a-z0-9]{6,20})\b/gi,
    (value) =>
      /^(bank|card|account|alert|info)$/i.test(value) ? null : { type: 'ref', label: `Ref: ${value}`, value }
  );

  // Merchants (e.g. spent at SWIGGY, paid to AMAZON)
  const merchants = scanCandidates(
    text,
    /(?:spent\s+(?:at|on)|paid\s+to|info[:\s]+|towards\s+)([A-Z0-9\s&]{3,20})/gi,
    (value) =>
      value.length >= 3 && !/^(your|bank|account|card|debit|credit)$/i.test(value)
        ? { type: 'merchant', label: `Merchant: ${value}`, value }
        : null,
    (raw) => raw.trim()
  );

  return [...amounts, ...dates, ...refs, ...merchants].slice(0, 6); // Top 6 relevant candidates
}

// "alerts@hdfcbank.net" -> "Hdfcbank Bank". Empty string when no usable domain.
function bankNameFromEmailDomain(address: string): string {
  const mainDomain = (address.split('@')[1] || '').split('.')[0] || '';
  return mainDomain ? `${mainDomain.charAt(0).toUpperCase()}${mainDomain.slice(1)} Bank` : '';
}

/**
 * Parses RFC 822 sender strings like "IndusInd Bank <indusind_bank@indusind.com>" or "alerts@indusind.com"
 * into separate clean display name and email address.
 * Falls back to scanning email body text/HTML for sender emails and bank names if headers are missing.
 */
const BANK_KEYWORDS = [
  'HDFC Bank',
  'IndusInd Bank',
  'ICICI Bank',
  'Axis Bank',
  'State Bank of India',
  'SBI Bank',
  'Kotak Mahindra Bank',
  'Citibank',
  'Standard Chartered',
  'Canara Bank',
  'Union Bank',
  'Yes Bank',
  'Paytm',
  'PhonePe',
  'Google Pay',
];

// Names the upstream extractor emits when it could not identify the sender.
const isPlaceholderName = (name: string) => name === 'Bank Alert' || name === 'Bank / Service Alert';

// "IndusInd Bank <alerts@indusind.com>" -> name and address. A sender without
// angle brackets, or one that does not parse, comes back as the name alone.
function splitRfc822Sender(raw: string): { name: string; email: string } {
  if (!raw.includes('<') || !raw.includes('>')) return { name: raw, email: '' };
  const match = raw.match(/^(.*?)\s*<([^>]+)>/);
  if (!match) return { name: raw, email: '' };
  return { name: match[1].replace(/^["']|["']$/g, '').trim(), email: match[2].trim() };
}

// First address in the body that is not a schema.org/example.com placeholder.
function findEmailInBody(body: string): string {
  const matches = body.match(/\b[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}\b/g);
  return matches?.find((e) => !e.includes('example.com') && !e.includes('schema.org')) || '';
}

function findBankNameInBody(body: string): string {
  return BANK_KEYWORDS.find((kw) => new RegExp(`\\b${kw}\\b`, 'i').test(body)) || '';
}

// Second pass for when the headers gave us nothing usable: mine the body for an
// address and a bank name, falling back to the address domain for the name.
function recoverIdentityFromBody(name: string, email: string, body: string): { name: string; email: string } {
  const foundEmail = email || findEmailInBody(body);
  if (name && !isPlaceholderName(name)) return { name, email: foundEmail };

  let foundName = findBankNameInBody(body) || name;
  // Note: only 'Bank Alert' retries here, not every placeholder name.
  if ((!foundName || foundName === 'Bank Alert') && foundEmail) {
    foundName = bankNameFromEmailDomain(foundEmail) || foundName;
  }
  return { name: foundName, email: foundEmail };
}

export function parseSenderInfo(rawSender?: string | null, rawEmail?: string | null, contentText?: string | null) {
  const fromHeader = splitRfc822Sender(rawSender?.trim() || '');
  let name = fromHeader.name;
  let email = rawEmail?.trim() || fromHeader.email;

  // Sender header carried a bare address rather than a display name.
  if (name.includes('@') && !email) {
    email = name;
    name = bankNameFromEmailDomain(name) || email;
  }

  if ((!email || isPlaceholderName(name)) && contentText) {
    ({ name, email } = recoverIdentityFromBody(name, email, contentText));
  }

  if (!name && email) {
    name = email.split('@')[0];
  }

  return {
    displayName: name || 'Bank Alert',
    displayEmail: email || (name ? `${name.toLowerCase().replace(/\s+/g, '')}@bank.com` : ''),
  };
}
