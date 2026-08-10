/**
 * Shows the source email beside the password prompt.
 *
 * Deliberate: the password rule is usually stated in the email itself, so showing
 * it turns an unanswerable prompt into an answerable one.
 */
import { GmailEmailViewer } from '@/components/common/GmailEmailViewer';
import type { StatementEmailContext } from './useStatementPasswordPrompt';

/** Splits a sender header into name and address. */
function splitSender(sender: string | undefined | null) {
  const name = sender?.split('<')[0]?.trim() || 'Unknown Sender';
  const address = sender?.includes('<') ? `<${sender.split('<')[1]}` : '';
  return { name, address, initial: sender ? sender.charAt(0).toUpperCase() : '?' };
}

/** Sender identity above the message body. */
function SenderHeader({ details }: { details: StatementEmailContext | null }) {
  const { name, address, initial } = splitSender(details?.sender);
  const received = details?.date
    ? new Date(details.date).toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' })
    : '';

  return (
    <div className="px-8 flex items-start justify-between pb-6 min-w-0 gap-4">
      <div className="flex items-center gap-4 min-w-0">
        <div className="w-12 h-12 rounded-full bg-[#1a73e8] flex items-center justify-center text-white font-medium shrink-0 text-xl">
          {initial}
        </div>
        <div className="flex flex-col min-w-0">
          <div className="flex items-baseline gap-1.5 flex-wrap min-w-0">
            <span className="font-bold text-sm text-gray-900 truncate">{name}</span>
            <span className="text-xs text-gray-500 truncate">{address}</span>
          </div>
          <div className="text-xs text-gray-500 flex items-center gap-1 mt-0.5">
            to me
            <svg className="w-3 h-3 text-gray-400" fill="currentColor" viewBox="0 0 24 24">
              <path d="M7 10l5 5 5-5z" />
            </svg>
          </div>
        </div>
      </div>
      <div className="text-xs text-gray-500 whitespace-nowrap mt-1 shrink-0">{received}</div>
    </div>
  );
}

/** Shows the source email beside the password prompt. */
export default function EmailContextPane({ details }: { details: StatementEmailContext | null }) {
  return (
    <div className="bg-[#F3EBDD] text-black border-r flex flex-col min-h-0 min-w-0 h-full">
      <div className="px-8 pt-8 pb-4 min-w-0">
        <h2 className="text-2xl font-normal leading-tight text-gray-900 break-words">
          {details?.subject || 'Statement Context'}
        </h2>
      </div>

      <SenderHeader details={details} />

      <div className="flex-1 min-h-0 min-w-0 px-8 pb-8 flex flex-col">
        <GmailEmailViewer
          html={details?.html}
          text={details?.snippet}
          showHeader={false}
          showViewModeSwitcher={true}
          className="flex-1 border-slate-200/60 shadow-xs min-w-0 h-full"
          maxHeight="100%"
        />
      </div>
    </div>
  );
}
