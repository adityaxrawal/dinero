/**
 * Shows the record of consents given and withdrawn.
 *
 * Read directly from the backend's durable consent events, so this reflects what
 * was actually recorded rather than a UI-side approximation of it.
 */
import { Loader2, RefreshCw } from 'lucide-react';
import type { ConsentEventRecord } from '@/lib/ipc';

/** Lists consents given and withdrawn, read from the durable event log. */
export default function ConsentHistoryList({
  events,
  isLoading,
  onRefresh,
}: {
  events: ConsentEventRecord[];
  isLoading: boolean;
  onRefresh: () => void;
}) {
  return (
    <div>
      <div className="flex items-center justify-between gap-4 mb-4">
        <p className="text-[13px] font-medium text-[#064E3B]/70">
          A log of all significant local data changes, API key operations, and account connections.
        </p>
        <button
          className="h-8 px-3 text-[12px] font-semibold rounded-lg border border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5 transition-colors flex items-center justify-center gap-1.5 shrink-0"
          onClick={onRefresh}
          disabled={isLoading}
          aria-label="Refresh consent history"
        >
          {isLoading ? (
            <Loader2 className="w-3.5 h-3.5 animate-spin" />
          ) : (
            <RefreshCw className="w-3.5 h-3.5" />
          )}
        </button>
      </div>

      {isLoading ? (
        <p className="text-[13px] font-medium text-[#064E3B]/70">Loading…</p>
      ) : events.length === 0 ? (
        <p className="text-[13px] font-medium text-[#064E3B]/70">No consent events recorded yet.</p>
      ) : (
        <div className="flex flex-col gap-3 max-h-[280px] overflow-y-auto pr-2">
          {events.map((event) => (
            <div
              key={event.id}
              className="p-4 rounded-xl border border-[#064E3B]/10 bg-[#064E3B]/5"
            >
              <div className="flex justify-between gap-4 mb-1">
                <strong className="text-[14px] font-bold text-[#064E3B]">{event.event_type}</strong>
                <span className="text-[12px] font-semibold text-[#064E3B]/60 whitespace-nowrap">
                  {new Date(event.consented_at).toLocaleString()}
                </span>
              </div>
              <p className="text-[13px] font-medium text-[#064E3B]/80">{event.disclosure_text}</p>
              {event.withdrawn_at && (
                <p className="text-[12px] font-medium text-[#064E3B]/60 mt-1 italic">
                  Withdrawn {new Date(event.withdrawn_at).toLocaleString()}
                </p>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
