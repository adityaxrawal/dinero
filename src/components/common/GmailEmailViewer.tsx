import { useState, useRef, useMemo } from 'react';
import { Mail, Eye, Sparkles, Code, DollarSign, Calendar, Hash, Store, Zap } from 'lucide-react';
import { cn } from '@/lib/utils';
import { Badge } from '@/components/ui/badge';

export interface QuickFillData {
  field: 'amount' | 'merchant' | 'date' | 'referenceId';
  value: string;
}

interface GmailEmailViewerProps {
  html?: string | null | undefined;
  text?: string | null | undefined;
  sender?: string | null | undefined;
  senderEmail?: string | null | undefined;
  subject?: string | null | undefined;
  date?: string | null | undefined;
  showHeader?: boolean | undefined;
  showViewModeSwitcher?: boolean | undefined;
  initialViewMode?: 'reader' | 'html' | 'text' | undefined;
  className?: string | undefined;
  maxHeight?: string | number | undefined;
  onQuickFill?: (data: QuickFillData) => void;
}

// Generates a consistent Gmail-style avatar background color from sender name
function getAvatarColor(name: string): string {
  const colors = [
    '#1a73e8', // Gmail Blue
    '#ea4335', // Gmail Red
    '#fbbc04', // Gmail Yellow
    '#34a853', // Gmail Green
    '#9334e6', // Purple
    '#fa7b17', // Orange
    '#007b83', // Teal
  ];
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = name.charCodeAt(i) + ((hash << 5) - hash);
  }
  return colors[Math.abs(hash) % colors.length];
}

/**
 * Strips raw CSS rules, style blocks, and non-content code from string.
 */
export function sanitizeCssAndCode(input: string): string {
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
function prepareGmailHtml(rawHtml?: string | null, rawText?: string | null): string {
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
 * Extracts candidate financial entities from text content for Quick-Fill Chips.
 */
function extractQuickCandidates(text: string) {
  const candidates: { type: 'amount' | 'merchant' | 'date' | 'ref'; label: string; value: string }[] = [];
  if (!text) return candidates;

  // 1. Amounts (e.g. INR 450.00, Rs. 1,200, ₹500.00, 450.00 spent)
  const amountRegex = /(?:INR|RS\.?|₹)\s*([\d,]+(?:\.\d{1,2})?)/gi;
  let match;
  const foundAmounts = new Set<string>();
  while ((match = amountRegex.exec(text)) !== null) {
    const rawVal = match[1].replace(/,/g, '');
    const num = parseFloat(rawVal);
    if (!isNaN(num) && num > 0 && !foundAmounts.has(rawVal)) {
      foundAmounts.add(rawVal);
      candidates.push({
        type: 'amount',
        label: `Amount: ₹${num.toLocaleString('en-IN')}`,
        value: num.toFixed(2),
      });
    }
  }

  // 2. Dates (e.g. 26-Jul-2026, 2026-07-26, 26/07/2026)
  const dateRegex = /\b(\d{4}-\d{2}-\d{2}|\d{1,2}[-/\s](?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)[a-z]*[-/\s]\d{2,4})\b/gi;
  const foundDates = new Set<string>();
  while ((match = dateRegex.exec(text)) !== null) {
    const dStr = match[1];
    if (!foundDates.has(dStr)) {
      foundDates.add(dStr);
      try {
        const parsed = new Date(dStr);
        if (!isNaN(parsed.getTime())) {
          const formatted = parsed.toISOString().slice(0, 10);
          candidates.push({
            type: 'date',
            label: `Date: ${formatted}`,
            value: formatted,
          });
        }
      } catch {
        // ignore invalid dates
      }
    }
  }

  // 3. Reference IDs / Txn IDs (e.g. Ref No: 12345678, Txn ID: TXN987654)
  const refRegex = /\b(?:ref|reference|txn|transaction|utr|rrn)[\s#:]*([a-z0-9]{6,20})\b/gi;
  const foundRefs = new Set<string>();
  while ((match = refRegex.exec(text)) !== null) {
    const refVal = match[1];
    if (!foundRefs.has(refVal) && !/^(bank|card|account|alert|info)$/i.test(refVal)) {
      foundRefs.add(refVal);
      candidates.push({
        type: 'ref',
        label: `Ref: ${refVal}`,
        value: refVal,
      });
    }
  }

  // 4. Merchant Candidates (e.g. spent at SWIGGY, paid to AMAZON)
  const merchantRegex = /(?:spent\s+(?:at|on)|paid\s+to|info[:\s]+|towards\s+)([A-Z0-9\s&]{3,20})/gi;
  const foundMerchants = new Set<string>();
  while ((match = merchantRegex.exec(text)) !== null) {
    const name = match[1].trim();
    if (name && name.length >= 3 && !foundMerchants.has(name) && !/^(your|bank|account|card|debit|credit)$/i.test(name)) {
      foundMerchants.add(name);
      candidates.push({
        type: 'merchant',
        label: `Merchant: ${name}`,
        value: name,
      });
    }
  }

  return candidates.slice(0, 6); // Limit to top 6 relevant candidates
}

/**
 * Parses RFC 822 sender strings like "IndusInd Bank <indusind_bank@indusind.com>" or "alerts@indusind.com"
 * into separate clean display name and email address.
 * Falls back to scanning email body text/HTML for sender emails and bank names if headers are missing.
 */
function parseSenderInfo(rawSender?: string | null, rawEmail?: string | null, contentText?: string | null) {
  let name = rawSender?.trim() || '';
  let email = rawEmail?.trim() || '';

  if (name.includes('<') && name.includes('>')) {
    const match = name.match(/^(.*?)\s*<([^>]+)>/);
    if (match) {
      name = match[1].replace(/^["']|["']$/g, '').trim();
      if (!email) email = match[2].trim();
    }
  }

  if (name.includes('@') && !email) {
    email = name;
    const domain = name.split('@')[1] || '';
    const mainDomain = domain.split('.')[0] || '';
    if (mainDomain) {
      name = mainDomain.charAt(0).toUpperCase() + mainDomain.slice(1) + ' Bank';
    } else {
      name = email;
    }
  }

  // Smart Fallback: If email address is still missing or name is generic, scan body content
  if ((!email || name === 'Bank Alert' || name === 'Bank / Service Alert') && contentText) {
    // 1. Try to find any email address in the body content (e.g. alerts@hdfcbank.net)
    if (!email) {
      const emailMatches = contentText.match(/\b[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}\b/g);
      if (emailMatches && emailMatches.length > 0) {
        // Pick the first non-generic email address if possible
        const validEmail = emailMatches.find((e) => !e.includes('example.com') && !e.includes('schema.org'));
        if (validEmail) {
          email = validEmail;
        }
      }
    }

    // 2. Try to infer bank/service name from body text
    if (!name || name === 'Bank Alert' || name === 'Bank / Service Alert') {
      const bankKeywords = [
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
      for (const kw of bankKeywords) {
        if (new RegExp(`\\b${kw}\\b`, 'i').test(contentText)) {
          name = kw;
          break;
        }
      }

      // If bank name still not found, derive from email domain
      if ((!name || name === 'Bank Alert') && email) {
        const domain = email.split('@')[1] || '';
        const mainDomain = domain.split('.')[0] || '';
        if (mainDomain) {
          name = mainDomain.charAt(0).toUpperCase() + mainDomain.slice(1) + ' Bank';
        }
      }
    }
  }

  if (!name && email) {
    name = email.split('@')[0];
  }

  return {
    displayName: name || 'Bank Alert',
    displayEmail: email || (name ? `${name.toLowerCase().replace(/\s+/g, '')}@bank.com` : ''),
  };
}

export function GmailEmailViewer({
  html,
  text,
  sender,
  senderEmail,
  subject,
  date,
  showHeader = true,
  showViewModeSwitcher = true,
  initialViewMode,
  className,
  maxHeight = '480px',
  onQuickFill,
}: GmailEmailViewerProps) {
  const [viewMode, setViewMode] = useState<'reader' | 'html' | 'text'>(
    initialViewMode || (html && html.trim() ? 'html' : 'reader')
  );
  const [iframeHeight, setIframeHeight] = useState<number>(320);
  const iframeRef = useRef<HTMLIFrameElement>(null);

  const cleanText = useMemo(() => cleanTextForReader(html, text), [html, text]);
  const processedHtml = useMemo(() => prepareGmailHtml(html, text), [html, text]);
  const quickCandidates = useMemo(() => extractQuickCandidates(cleanText || subject || ''), [cleanText, subject]);

  const hasHtml = Boolean(html && html.trim());
  const { displayName, displayEmail } = useMemo(
    () => parseSenderInfo(sender, senderEmail, cleanText || html || ''),
    [sender, senderEmail, cleanText, html]
  );
  
  const avatarLetter = displayName.charAt(0).toUpperCase();
  const avatarBg = getAvatarColor(displayName);

  const formattedDate = useMemo(() => {
    if (!date) return '';
    try {
      const d = new Date(date);
      if (isNaN(d.getTime())) return date;
      return d.toLocaleString('en-US', {
        month: 'short',
        day: 'numeric',
        year: 'numeric',
        hour: 'numeric',
        minute: '2-digit',
        hour12: true,
      });
    } catch {
      return date;
    }
  }, [date]);

  const handleIframeLoad = () => {
    try {
      const doc = iframeRef.current?.contentDocument;
      if (doc?.body) {
        const scrollHeight = doc.body.scrollHeight;
        if (scrollHeight > 80) {
          setIframeHeight(Math.max(scrollHeight + 20, 260));
        }
      }
    } catch {
      // Ignore iframe cross-origin error
    }
  };

  const handleCandidateClick = (cand: { type: 'amount' | 'merchant' | 'date' | 'ref'; value: string }) => {
    if (!onQuickFill) return;
    const fieldMap: Record<string, 'amount' | 'merchant' | 'date' | 'referenceId'> = {
      amount: 'amount',
      merchant: 'merchant',
      date: 'date',
      ref: 'referenceId',
    };
    onQuickFill({
      field: fieldMap[cand.type],
      value: cand.value,
    });
  };

  return (
    <div className={cn('bg-white border border-[#064E3B]/15 rounded-xl shadow-xs overflow-hidden flex flex-col', className)}>
      {/* Full Gmail Header */}
      {showHeader && (
        <div className="bg-[#f6f8fc] px-4 py-3.5 border-b border-slate-200/80 flex flex-col gap-2.5 shrink-0">
          {/* Subject Row */}
          {subject && (
            <div className="text-[15px] font-semibold text-slate-900 tracking-tight flex items-center justify-between">
              <span className="truncate">{subject}</span>
              <span className="text-[10px] uppercase font-bold tracking-wider px-2 py-0.5 rounded bg-blue-50 text-blue-700 border border-blue-200/60 flex items-center gap-1 shrink-0 ml-2">
                <Mail className="w-3 h-3" /> Gmail
              </span>
            </div>
          )}

          {/* Sender & View Modes (Identical to Gmail layout) */}
          <div className="flex items-start justify-between gap-3">
            <div className="flex items-start gap-3 min-w-0">
              {/* Avatar Circle */}
              <div
                className="w-8 h-8 rounded-full flex items-center justify-center text-white font-bold text-sm shrink-0 shadow-xs mt-0.5"
                style={{ backgroundColor: avatarBg }}
              >
                {avatarLetter}
              </div>

              {/* Gmail Sender Info Header */}
              <div className="min-w-0 flex-1">
                <div className="flex items-baseline gap-1.5 flex-wrap">
                  <span className="font-bold text-[14px] text-slate-900 tracking-tight">
                    {displayName}
                  </span>
                  {displayEmail && (
                    <span className="text-[12px] text-slate-500 font-normal">
                      &lt;{displayEmail}&gt;
                    </span>
                  )}
                </div>
                <div className="text-[12px] text-slate-600 font-normal flex items-center gap-1 mt-0.5">
                  <span>to me</span>
                  <svg className="w-2.5 h-2.5 text-slate-500" fill="currentColor" viewBox="0 0 24 24">
                    <path d="M7 10l5 5 5-5z" />
                  </svg>
                </div>
              </div>
            </div>

            <div className="flex items-center gap-2.5 shrink-0 pt-0.5">
              {formattedDate && (
                <span className="text-[12px] text-slate-500 font-normal hidden sm:inline">
                  {formattedDate}
                </span>
              )}

              {/* View Mode Segmented Switcher */}
              <div className="flex items-center bg-slate-200/60 rounded-lg p-0.5">
                <button
                  type="button"
                  onClick={() => setViewMode('reader')}
                  className={cn(
                    'px-2.5 py-1 text-[11px] font-medium rounded-md transition-all flex items-center gap-1.5',
                    viewMode === 'reader'
                      ? 'bg-white text-slate-900 shadow-xs font-semibold'
                      : 'text-slate-600 hover:text-slate-900'
                  )}
                  title="View Cleaned Reader Text"
                >
                  <Sparkles className="w-3 h-3 text-amber-500" /> Reader View
                </button>
                {hasHtml && (
                  <button
                    type="button"
                    onClick={() => setViewMode('html')}
                    className={cn(
                      'px-2.5 py-1 text-[11px] font-medium rounded-md transition-all flex items-center gap-1.5',
                      viewMode === 'html'
                        ? 'bg-white text-slate-900 shadow-xs font-semibold'
                        : 'text-slate-600 hover:text-slate-900'
                    )}
                    title="View Original Gmail HTML"
                  >
                    <Eye className="w-3 h-3 text-blue-600" /> Gmail View
                  </button>
                )}
                <button
                  type="button"
                  onClick={() => setViewMode('text')}
                  className={cn(
                    'px-2.5 py-1 text-[11px] font-medium rounded-md transition-all flex items-center gap-1.5',
                    viewMode === 'text'
                      ? 'bg-white text-slate-900 shadow-xs font-semibold'
                      : 'text-slate-600 hover:text-slate-900'
                  )}
                  title="View Plain Text"
                >
                  <Code className="w-3 h-3" /> Text
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Compact View Mode Switcher when full showHeader is false */}
      {!showHeader && showViewModeSwitcher && (
        <div className="bg-[#f6f8fc] px-4 py-2 border-b border-slate-200/80 flex items-center justify-between shrink-0">
          <span className="text-[12px] font-medium text-slate-600 flex items-center gap-1.5">
            <Mail className="w-3.5 h-3.5 text-slate-500" /> View Mode
          </span>
          {/* View Mode Segmented Switcher */}
          <div className="flex items-center bg-slate-200/60 rounded-lg p-0.5">
            <button
              type="button"
              onClick={() => setViewMode('reader')}
              className={cn(
                'px-2.5 py-1 text-[11px] font-medium rounded-md transition-all flex items-center gap-1.5',
                viewMode === 'reader'
                  ? 'bg-white text-slate-900 shadow-xs font-semibold'
                  : 'text-slate-600 hover:text-slate-900'
              )}
              title="View Cleaned Reader Text"
            >
              <Sparkles className="w-3 h-3 text-amber-500" /> Reader View
            </button>
            {hasHtml && (
              <button
                type="button"
                onClick={() => setViewMode('html')}
                className={cn(
                  'px-2.5 py-1 text-[11px] font-medium rounded-md transition-all flex items-center gap-1.5',
                  viewMode === 'html'
                    ? 'bg-white text-slate-900 shadow-xs font-semibold'
                    : 'text-slate-600 hover:text-slate-900'
                )}
                title="View Original Gmail HTML"
              >
                <Eye className="w-3 h-3 text-blue-600" /> Gmail View
              </button>
            )}
            <button
              type="button"
              onClick={() => setViewMode('text')}
              className={cn(
                'px-2.5 py-1 text-[11px] font-medium rounded-md transition-all flex items-center gap-1.5',
                viewMode === 'text'
                  ? 'bg-white text-slate-900 shadow-xs font-semibold'
                  : 'text-slate-600 hover:text-slate-900'
              )}
              title="View Plain Text"
            >
              <Code className="w-3 h-3" /> Text
            </button>
          </div>
        </div>
      )}

      {/* Quick-Fill Suggestion Chips Bar */}
      {onQuickFill && quickCandidates.length > 0 && (
        <div className="bg-amber-50/60 px-4 py-2 border-b border-amber-200/60 flex items-center gap-2 flex-wrap">
          <span className="text-[11px] font-bold text-amber-900 flex items-center gap-1 uppercase tracking-wider shrink-0">
            <Zap className="w-3 h-3 text-amber-600 fill-amber-500" /> Quick-Fill:
          </span>
          {quickCandidates.map((cand, idx) => {
            const iconMap = {
              amount: <DollarSign className="w-3 h-3 text-emerald-600" />,
              date: <Calendar className="w-3 h-3 text-blue-600" />,
              ref: <Hash className="w-3 h-3 text-purple-600" />,
              merchant: <Store className="w-3 h-3 text-amber-600" />,
            };
            return (
              <Badge
                key={idx}
                variant="outline"
                onClick={() => handleCandidateClick(cand)}
                className="bg-white hover:bg-amber-100 text-slate-800 border-amber-300/80 cursor-pointer transition-colors text-[11px] py-0.5 px-2 font-medium flex items-center gap-1 shadow-xs hover:border-amber-400"
                title={`Click to auto-fill ${cand.type} into form`}
              >
                {iconMap[cand.type]}
                <span>{cand.label}</span>
              </Badge>
            );
          })}
        </div>
      )}

      {/* Content Canvas */}
      <div className="bg-white flex-1 overflow-y-auto min-h-0" style={{ maxHeight }}>
        {viewMode === 'reader' ? (
          <div className="p-4 sm:p-5 text-[13px] text-slate-800 leading-relaxed font-sans">
            {cleanText ? (
              <div className="space-y-3 whitespace-pre-wrap leading-relaxed text-[#064E3B]">
                {cleanText}
              </div>
            ) : (
              <p className="text-slate-400 italic text-xs">No email body text available.</p>
            )}
          </div>
        ) : viewMode === 'html' && hasHtml ? (
          <iframe
            ref={iframeRef}
            srcDoc={processedHtml}
            onLoad={handleIframeLoad}
            title="Gmail Email Content"
            sandbox="allow-same-origin allow-popups"
            className="w-full border-0 block bg-white"
            style={{ height: iframeHeight, minHeight: '100%' }}
          />
        ) : (
          <div className="p-4 sm:p-5 bg-slate-50/50">
            <pre className="text-[11px] font-mono text-slate-700 leading-relaxed whitespace-pre-wrap break-words">
              {text || cleanText || 'No email content available.'}
            </pre>
          </div>
        )}
      </div>
    </div>
  );
}
