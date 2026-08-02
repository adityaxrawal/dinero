import { useState, useRef, useMemo, type ReactNode } from 'react';
import { Mail, Eye, Sparkles, Code, DollarSign, Calendar, Hash, Store, Zap } from 'lucide-react';
import { cn } from '@/lib/utils';
import { Badge } from '@/components/ui/badge';
import {
  cleanTextForReader,
  prepareGmailHtml,
  extractQuickCandidates,
  parseSenderInfo,
  type QuickCandidate,
} from './gmailParsing';

interface QuickFillData {
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


type ViewMode = 'reader' | 'html' | 'text';

function ViewModeSwitcher({
  viewMode,
  setViewMode,
  hasHtml,
}: {
  viewMode: ViewMode;
  setViewMode: (mode: ViewMode) => void;
  hasHtml: boolean;
}) {
  const tabClass = (active: boolean) =>
    cn(
      'px-2.5 py-1 text-[11px] font-medium rounded-md transition-all flex items-center gap-1.5',
      active ? 'bg-white text-slate-900 shadow-xs font-semibold' : 'text-slate-600 hover:text-slate-900'
    );

  return (
    <div className="flex items-center bg-slate-200/60 rounded-lg p-0.5">
      <button
        type="button"
        onClick={() => setViewMode('reader')}
        className={tabClass(viewMode === 'reader')}
        title="View Cleaned Reader Text"
      >
        <Sparkles className="w-3 h-3 text-amber-500" /> Reader View
      </button>
      {hasHtml && (
        <button
          type="button"
          onClick={() => setViewMode('html')}
          className={tabClass(viewMode === 'html')}
          title="View Original Gmail HTML"
        >
          <Eye className="w-3 h-3 text-blue-600" /> Gmail View
        </button>
      )}
      <button
        type="button"
        onClick={() => setViewMode('text')}
        className={tabClass(viewMode === 'text')}
        title="View Plain Text"
      >
        <Code className="w-3 h-3" /> Text
      </button>
    </div>
  );
}

function formatEmailDate(date?: string | null): string {
  if (!date) return '';
  const parsed = new Date(date);
  if (isNaN(parsed.getTime())) return date;
  return parsed.toLocaleString('en-US', {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
    hour12: true,
  });
}

const QUICK_FILL_ICONS: Record<QuickCandidate['type'], ReactNode> = {
  amount: <DollarSign className="w-3 h-3 text-emerald-600" />,
  date: <Calendar className="w-3 h-3 text-blue-600" />,
  ref: <Hash className="w-3 h-3 text-purple-600" />,
  merchant: <Store className="w-3 h-3 text-amber-600" />,
};

const QUICK_FILL_FIELDS: Record<QuickCandidate['type'], QuickFillData['field']> = {
  amount: 'amount',
  merchant: 'merchant',
  date: 'date',
  ref: 'referenceId',
};

function GmailHeader({
  subject,
  displayName,
  displayEmail,
  formattedDate,
  viewMode,
  setViewMode,
  hasHtml,
}: {
  subject?: string | null | undefined;
  displayName: string;
  displayEmail: string;
  formattedDate: string;
  viewMode: ViewMode;
  setViewMode: (mode: ViewMode) => void;
  hasHtml: boolean;
}) {
  return (
    <div className="bg-[#f6f8fc] px-4 py-3.5 border-b border-slate-200/80 flex flex-col gap-2.5 shrink-0">
      {subject && (
        <div className="text-[15px] font-semibold text-slate-900 tracking-tight flex items-center justify-between">
          <span className="truncate">{subject}</span>
          <span className="text-[10px] uppercase font-bold tracking-wider px-2 py-0.5 rounded bg-blue-50 text-blue-700 border border-blue-200/60 flex items-center gap-1 shrink-0 ml-2">
            <Mail className="w-3 h-3" /> Gmail
          </span>
        </div>
      )}

      {/* Sender & View Modes (identical to Gmail layout) */}
      <div className="flex items-start justify-between gap-3">
        <div className="flex items-start gap-3 min-w-0">
          <div
            className="w-8 h-8 rounded-full flex items-center justify-center text-white font-bold text-sm shrink-0 shadow-xs mt-0.5"
            style={{ backgroundColor: getAvatarColor(displayName) }}
          >
            {displayName.charAt(0).toUpperCase()}
          </div>

          <div className="min-w-0 flex-1">
            <div className="flex items-baseline gap-1.5 flex-wrap">
              <span className="font-bold text-[14px] text-slate-900 tracking-tight">{displayName}</span>
              {displayEmail && (
                <span className="text-[12px] text-slate-500 font-normal">&lt;{displayEmail}&gt;</span>
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
            <span className="text-[12px] text-slate-500 font-normal hidden sm:inline">{formattedDate}</span>
          )}
          <ViewModeSwitcher viewMode={viewMode} setViewMode={setViewMode} hasHtml={hasHtml} />
        </div>
      </div>
    </div>
  );
}

function QuickFillBar({
  candidates,
  onQuickFill,
}: {
  candidates: QuickCandidate[];
  onQuickFill: (data: QuickFillData) => void;
}) {
  if (candidates.length === 0) return null;
  return (
    <div className="bg-amber-50/60 px-4 py-2 border-b border-amber-200/60 flex items-center gap-2 flex-wrap">
      <span className="text-[11px] font-bold text-amber-900 flex items-center gap-1 uppercase tracking-wider shrink-0">
        <Zap className="w-3 h-3 text-amber-600 fill-amber-500" /> Quick-Fill:
      </span>
      {candidates.map((cand, idx) => (
        <Badge
          key={idx}
          variant="outline"
          onClick={() => onQuickFill({ field: QUICK_FILL_FIELDS[cand.type], value: cand.value })}
          className="bg-white hover:bg-amber-100 text-slate-800 border-amber-300/80 cursor-pointer transition-colors text-[11px] py-0.5 px-2 font-medium flex items-center gap-1 shadow-xs hover:border-amber-400"
          title={`Click to auto-fill ${cand.type} into form`}
        >
          {QUICK_FILL_ICONS[cand.type]}
          <span>{cand.label}</span>
        </Badge>
      ))}
    </div>
  );
}

function EmailBody({
  viewMode,
  hasHtml,
  cleanText,
  processedHtml,
  text,
}: {
  viewMode: ViewMode;
  hasHtml: boolean;
  cleanText: string;
  processedHtml: string;
  text?: string | null | undefined;
}) {
  const [iframeHeight, setIframeHeight] = useState<number>(320);
  const iframeRef = useRef<HTMLIFrameElement>(null);

  // Grow the iframe to its content so the outer container does the scrolling.
  const handleIframeLoad = () => {
    try {
      const scrollHeight = iframeRef.current?.contentDocument?.body?.scrollHeight;
      if (scrollHeight && scrollHeight > 80) {
        setIframeHeight(Math.max(scrollHeight + 20, 260));
      }
    } catch {
      // Ignore iframe cross-origin error
    }
  };

  if (viewMode === 'reader') {
    return (
      <div className="p-4 sm:p-5 text-[13px] text-slate-800 leading-relaxed font-sans">
        {cleanText ? (
          <div className="space-y-3 whitespace-pre-wrap leading-relaxed text-[#064E3B]">{cleanText}</div>
        ) : (
          <p className="text-slate-400 italic text-xs">No email body text available.</p>
        )}
      </div>
    );
  }

  if (viewMode === 'html' && hasHtml) {
    return (
      <iframe
        ref={iframeRef}
        srcDoc={processedHtml}
        onLoad={handleIframeLoad}
        title="Gmail Email Content"
        sandbox="allow-same-origin allow-popups"
        className="w-full border-0 block bg-white"
        style={{ height: iframeHeight, minHeight: '100%' }}
      />
    );
  }

  return (
    <div className="p-4 sm:p-5 bg-slate-50/50">
      <pre className="text-[11px] font-mono text-slate-700 leading-relaxed whitespace-pre-wrap break-words">
        {text || cleanText || 'No email content available.'}
      </pre>
    </div>
  );
}

/**
 * Whatever sits above the message body: the full Gmail header, or — when the
 * caller hides it — a compact bar carrying just the view-mode switcher.
 */
function ViewerChrome({
  showHeader,
  showViewModeSwitcher,
  header,
  viewMode,
  setViewMode,
  hasHtml,
}: {
  showHeader: boolean;
  showViewModeSwitcher: boolean;
  header: { subject?: string | null | undefined; displayName: string; displayEmail: string; formattedDate: string };
  viewMode: ViewMode;
  setViewMode: (mode: ViewMode) => void;
  hasHtml: boolean;
}) {
  if (showHeader) {
    return <GmailHeader {...header} viewMode={viewMode} setViewMode={setViewMode} hasHtml={hasHtml} />;
  }
  if (!showViewModeSwitcher) return null;
  return (
    <div className="bg-[#f6f8fc] px-4 py-2 border-b border-slate-200/80 flex items-center justify-between shrink-0">
      <span className="text-[12px] font-medium text-slate-600 flex items-center gap-1.5">
        <Mail className="w-3.5 h-3.5 text-slate-500" /> View Mode
      </span>
      <ViewModeSwitcher viewMode={viewMode} setViewMode={setViewMode} hasHtml={hasHtml} />
    </div>
  );
}

const defaultViewMode = (hasHtml: boolean): ViewMode => (hasHtml ? 'html' : 'reader');

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
  const hasHtml = Boolean(html?.trim());
  const [viewMode, setViewMode] = useState<ViewMode>(initialViewMode ?? defaultViewMode(hasHtml));

  const cleanText = useMemo(() => cleanTextForReader(html, text), [html, text]);
  const processedHtml = useMemo(() => prepareGmailHtml(html, text), [html, text]);
  const quickCandidates = useMemo(() => extractQuickCandidates(cleanText || subject || ''), [cleanText, subject]);
  const senderInfo = useMemo(
    () => parseSenderInfo(sender, senderEmail, cleanText || html || ''),
    [sender, senderEmail, cleanText, html]
  );

  return (
    <div className={cn('bg-white border border-[#064E3B]/15 rounded-xl shadow-xs overflow-hidden flex flex-col', className)}>
      <ViewerChrome
        showHeader={showHeader}
        showViewModeSwitcher={showViewModeSwitcher}
        header={{ subject, ...senderInfo, formattedDate: formatEmailDate(date) }}
        viewMode={viewMode}
        setViewMode={setViewMode}
        hasHtml={hasHtml}
      />

      {onQuickFill && <QuickFillBar candidates={quickCandidates} onQuickFill={onQuickFill} />}

      <div className="bg-white flex-1 overflow-y-auto min-h-0" style={{ maxHeight }}>
        <EmailBody
          viewMode={viewMode}
          hasHtml={hasHtml}
          cleanText={cleanText}
          processedHtml={processedHtml}
          text={text}
        />
      </div>
    </div>
  );
}
