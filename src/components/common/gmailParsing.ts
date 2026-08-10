
/**
 * Pure parsing helpers behind the email viewer.
 *
 * Kept out of the component file so the module exports components alone (which
 * Fast Refresh requires) and so this logic stays directly testable.
 */
/** One value spotted in an email body that the user can push into a form field. */
export type QuickCandidate = { type: 'amount' | 'merchant' | 'date' | 'ref'; label: string; value: string };

/**
 * Strips CSS and script content out of text destined for the reader view.
 *
 * Bank emails are built as HTML tables with large embedded stylesheets, so
 * naively stripping tags leaves the CSS rules behind as text. Each pass removes
 * a different way that styling leaks through: whole <style> and <script>
 * elements, media queries, bare selector-and-brace blocks, and finally any
 * remaining brace block containing recognisably CSS property names.
 */
function sanitizeCssAndCode(input: string): string {
  if (!input) return '';
  
  let cleaned = input;
  cleaned = cleaned.replace(/<style[\s\S]*?<\/style>/gi, ' ');
  cleaned = cleaned.replace(/<script[\s\S]*?<\/script>/gi, ' ');
  cleaned = cleaned.replace(/@media[^{]+\{(?:[^{}]*|\{[^{}]*\})*\}/gi, ' ');
  cleaned = cleaned.replace(/(?:^|\n)\s*(?:[a-z0-9_#.:\-\s,>+*()]+\s*\{[^}]*\})/gi, ' ');
  cleaned = cleaned.replace(/\{[^}]*(?:margin|padding|color|font-|border-|display:|text-align|width:|height:|background)[^}]*\}/gi, ' ');
  
  return cleaned;
}

/**
 * Produces readable plain text from an email, for the reader view.
 *
 * Prefers the plain-text part when the message provides one, since it is already
 * what the sender intended to be read. Falling back to HTML, structural tags are
 * converted to line breaks before the remaining tags are dropped -- stripping
 * tags first would run every table cell together into one unreadable line.
 *
 * Blank lines are collapsed and paragraphs double-spaced so the result is
 * scannable rather than a wall of text.
 */
export function cleanTextForReader(rawHtml?: string | null, rawText?: string | null): string {
  let textContent = '';

  if (rawText && rawText.trim()) {
    textContent = rawText;
  } else if (rawHtml && rawHtml.trim()) {
    textContent = rawHtml.replace(/<br\s*\/?>/gi, '\n')
                         .replace(/<\/p>/gi, '\n\n')
                         .replace(/<\/tr>/gi, '\n')
                         .replace(/<\/td>/gi, '  ')
                         .replace(/<[^>]+>/g, '');
  }

  textContent = sanitizeCssAndCode(textContent);

  return textContent
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .join('\n\n');
}

/**
 * Wraps an email's HTML in a self-contained document for the sandboxed iframe.
 *
 * Three problems are handled here. A message may carry its HTML in the text part
 * rather than the HTML part, so that case is detected and used. Some senders
 * emit their CSS outside any <style> element, where it would render as visible
 * text -- that CSS is extracted and re-wrapped properly. And the surrounding
 * document supplies resets that keep an email designed for a full-width client
 * from overflowing a narrow panel.
 *
 * `base target="_blank"` ensures any link the user clicks leaves the iframe
 * rather than navigating it in place.
 */
export function prepareGmailHtml(rawHtml?: string | null, rawText?: string | null): string {
  let content = rawHtml || '';

  if (!content.trim() && rawText) {
    const isHtmlTag = /<(table|tr|td|th|div|p|span|b|strong|em|i|u|h[1-6]|ul|ol|li|br|hr|html|body|head|header|footer|section|article|a|img|pre|code)[^>]*>/i.test(rawText);
    if (isHtmlTag) {
      content = rawText;
    }
  }

  if (!content.trim()) {
    return '';
  }

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
 * Runs one detection pattern over the text and builds deduplicated candidates.
 *
 * Shared by every quick-fill detector below. The `build` callback may return
 * null to reject a match that fits the pattern but is not actually useful, and
 * `normalize` canonicalises the captured text before the duplicate check -- so
 * that "1,200" and "1200" are recognised as the same amount rather than offered
 * twice.
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
 * Finds amounts, dates, references and merchants a user might want to fill in.
 *
 * Each detector is deliberately conservative and rejects its own false
 * positives: non-positive amounts, unparseable dates, reference captures that
 * are really common words, and merchant captures that are generic banking terms.
 * A wrong suggestion is worse than a missing one, because the user may accept it
 * without checking.
 *
 * The result is capped, since a long strip of chips is harder to scan than a
 * short one and defeats the purpose.
 */
export function extractQuickCandidates(text: string): QuickCandidate[] {
  if (!text) return [];

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

  const refs = scanCandidates(
    text,
    /\b(?:ref|reference|txn|transaction|utr|rrn)[\s#:]*([a-z0-9]{6,20})\b/gi,
    (value) =>
      /^(bank|card|account|alert|info)$/i.test(value) ? null : { type: 'ref', label: `Ref: ${value}`, value }
  );

  const merchants = scanCandidates(
    text,
    /(?:spent\s+(?:at|on)|paid\s+to|info[:\s]+|towards\s+)([A-Z0-9\s&]{3,20})/gi,
    (value) =>
      value.length >= 3 && !/^(your|bank|account|card|debit|credit)$/i.test(value)
        ? { type: 'merchant', label: `Merchant: ${value}`, value }
        : null,
    (raw) => raw.trim()
  );

  return [...amounts, ...dates, ...refs, ...merchants].slice(0, 6);
}

/**
 * Derives a display name from an email domain, as a last resort.
 *
 * Produces something like "Hdfc Bank" from the domain's first label. Crude, but
 * strictly better than showing the user a bare address.
 */
function bankNameFromEmailDomain(address: string): string {
  const mainDomain = (address.split('@')[1] || '').split('.')[0] || '';
  return mainDomain ? `${mainDomain.charAt(0).toUpperCase()}${mainDomain.slice(1)} Bank` : '';
}

// Institution names looked for in the message body when the headers are
// unhelpful. Longer names precede shorter ones that they contain, so the more
// specific match wins.
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

/** Whether a name is one of our own fallbacks rather than a real sender name. */
const isPlaceholderName = (name: string) => name === 'Bank Alert' || name === 'Bank / Service Alert';

/**
 * Splits a `Display Name <address@host>` header into its two parts.
 *
 * Returns the whole string as the name when there are no angle brackets, since
 * a bare address is still the best label available. Surrounding quotes are
 * stripped, as senders commonly quote names containing commas.
 */
function splitRfc822Sender(raw: string): { name: string; email: string } {
  if (!raw.includes('<') || !raw.includes('>')) return { name: raw, email: '' };
  const match = raw.match(/^(.*?)\s*<([^>]+)>/);
  if (!match) return { name: raw, email: '' };
  return { name: match[1].replace(/^["']|["']$/g, '').trim(), email: match[2].trim() };
}

/**
 * Finds a plausible sender address inside the message body.
 *
 * Skips example.com and schema.org, which appear in boilerplate and structured
 * metadata rather than belonging to the actual sender.
 */
function findEmailInBody(body: string): string {
  const matches = body.match(/\b[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}\b/g);
  return matches?.find((e) => !e.includes('example.com') && !e.includes('schema.org')) || '';
}

/** First known institution name mentioned in the body, if any. */
function findBankNameInBody(body: string): string {
  return BANK_KEYWORDS.find((kw) => new RegExp(`\\b${kw}\\b`, 'i').test(body)) || '';
}

/**
 * Recovers sender identity from the body when the headers were insufficient.
 *
 * Ordered by reliability: a real name already in the header is kept, otherwise a
 * recognised institution name in the body is preferred, and only then is the
 * email domain used. This runs because forwarded and relayed bank alerts
 * frequently lose their original From header.
 */
function recoverIdentityFromBody(name: string, email: string, body: string): { name: string; email: string } {
  const foundEmail = email || findEmailInBody(body);
  if (name && !isPlaceholderName(name)) return { name, email: foundEmail };

  let foundName = findBankNameInBody(body) || name;
  if ((!foundName || foundName === 'Bank Alert') && foundEmail) {
    foundName = bankNameFromEmailDomain(foundEmail) || foundName;
  }
  return { name: foundName, email: foundEmail };
}

/**
 * Resolves the sender's display name and address from whatever is available.
 *
 * Works through progressively weaker sources -- the From header, then an
 * explicitly supplied address, then the body -- and handles the common case of a
 * header carrying only an address with no name.
 *
 * Always returns something displayable, falling back to a generic label rather
 * than an empty string, because this feeds a UI that must render regardless.
 */
export function parseSenderInfo(rawSender?: string | null, rawEmail?: string | null, contentText?: string | null) {
  const fromHeader = splitRfc822Sender(rawSender?.trim() || '');
  let name = fromHeader.name;
  let email = rawEmail?.trim() || fromHeader.email;

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
