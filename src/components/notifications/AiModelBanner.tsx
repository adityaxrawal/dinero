/**
 * Tells the user when the local AI can't run because no model is installed,
 * with a one-click download, and shows download progress (owner decision Q-1:
 * the fallback when onboarding's download was skipped or failed).
 */
import { useState } from 'react';
import { Cpu } from 'lucide-react';
import { useLlmQueueStatus } from '@/hooks/queries/useLlmQueueStatus';
import { downloadPercent, useModelDownload } from '@/hooks/useModelDownload';
import { useToast } from '@/hooks/use-toast';
import { getErrorToast } from '@/lib/errorMapping';

export default function AiModelBanner() {
  const { data: queue } = useLlmQueueStatus();
  const { progress, start } = useModelDownload();
  const { toast } = useToast();
  const [starting, setStarting] = useState(false);

  if (progress) {
    const pct = downloadPercent(progress);
    return (
      <div role="status" className="mx-4 mb-2 rounded-lg border border-champagne/20 bg-champagne/10 px-3 py-2.5 text-[11.5px] text-champagne">
        Downloading the AI model{pct !== null ? `… ${pct}%` : '…'}
      </div>
    );
  }
  if (queue?.state !== 'no_model') return null;

  const download = () => {
    setStarting(true);
    start()
      .then(() => toast({ title: 'AI model ready', description: 'Waiting emails will be read shortly.' }))
      .catch((err) => toast({ variant: 'destructive', ...getErrorToast(err) }))
      .finally(() => setStarting(false));
  };

  return (
    <div role="status" className="mx-4 mb-2 flex items-start gap-2 rounded-lg border border-amber-400/30 bg-amber-400/10 px-3 py-2.5">
      <Cpu className="mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-300" aria-hidden="true" />
      <div className="flex-1 text-[11.5px] leading-snug text-amber-200">
        No AI model installed.
        {queue.pending > 0 ? ` ${queue.pending} email${queue.pending === 1 ? ' is' : 's are'} waiting for it.` : ''}{' '}
        <button type="button" onClick={download} disabled={starting} className="underline font-semibold">
          {starting ? 'Starting…' : 'Download'}
        </button>
      </div>
    </div>
  );
}
