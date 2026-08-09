import { Server, Settings, Pause, Play } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { cn } from '@/lib/utils';
import { useDebugMetrics } from './useDebugMetrics';

function PipelineControlCard({
  icon: Icon,
  title,
  description,
  paused,
  actionNoun,
  onToggle,
}: {
  icon: LucideIcon;
  title: string;
  description: string;
  paused: boolean;
  actionNoun: string;
  onToggle: () => void;
}) {
  return (
    <div className="bg-[#F8E7C9]/50 border border-[#064E3B]/10 rounded-xl p-6 flex flex-col gap-4">
      <div className="flex justify-between items-center">
        <h3 className="font-semibold flex items-center gap-2 text-[14px]">
          <Icon size={18} /> {title}
        </h3>
        <span
          className={cn(
            'text-[10px] uppercase tracking-wider px-2 py-0.5 rounded-sm font-bold',
            paused
              ? 'bg-amber-500/20 text-amber-700'
              : 'bg-[#064E3B]/10 text-[#064E3B]'
          )}
        >
          {paused ? 'PAUSED' : 'RUNNING'}
        </span>
      </div>
      <p className="text-[12px] text-[#064E3B]/70">{description}</p>
      <button
        className={cn(
          'h-8 rounded-lg text-[13px] font-semibold flex items-center justify-center gap-2 transition-colors mt-auto',
          paused
            ? 'bg-[#064E3B] text-[#F8E7C9] hover:bg-[#064E3B]/90'
            : 'bg-amber-500/20 text-amber-700 hover:bg-amber-500/30'
        )}
        onClick={onToggle}
      >
        {paused ? (
          <>
            <Play size={14} /> Resume {actionNoun}
          </>
        ) : (
          <>
            <Pause size={14} /> Pause {actionNoun}
          </>
        )}
      </button>
    </div>
  );
}

type Debug = ReturnType<typeof useDebugMetrics>;

export default function PipelineSection({ debug }: { debug: Debug }) {
  const { pipelineState } = debug;

  return (
    <div className="animate-in fade-in duration-300 flex flex-col gap-6">
      <h2 className="text-xl font-bold">Pipeline Controls</h2>
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <PipelineControlCard
          icon={Server}
          title="Transaction Polling"
          description="Controls the background polling of Gmail for new transaction emails."
          paused={pipelineState?.gmail_poll_paused ?? false}
          actionNoun="Polling"
          onToggle={debug.toggleGmailPoll}
        />
        <PipelineControlCard
          icon={Settings}
          title="Historical Scan Queue"
          description="Controls the processing of the historical scan queue for fetching emails."
          paused={pipelineState?.scan_queue_paused ?? false}
          actionNoun="Scan"
          onToggle={debug.toggleScanQueue}
        />
      </div>
    </div>
  );
}
