/**
 * Loads the raw metrics behind the debug screen.
 */
import { useEffect, useState } from 'react';
import { API, type DebugMetrics } from '@/lib/ipc';

interface PipelineState {
  gmail_poll_paused: boolean;
  scan_queue_paused: boolean;
}

const POLL_INTERVAL_MS = 15000;

/** Loads debug metrics and pipeline state. */
export function useDebugMetrics() {
  const [metrics, setMetrics] = useState<DebugMetrics | null>(null);
  const [pipelineState, setPipelineState] = useState<PipelineState | null>(null);
  const [ram, setRam] = useState<number | null>(null);

  /** Re-reads the metrics. */
  const refresh = () => {
    API.dev.getMetrics().then(setMetrics).catch(console.error);
    API.dev.checkSystemRam().then(setRam).catch(console.error);
    API.debug.getPipelineState().then(setPipelineState).catch(console.error);
  };

  useEffect(() => {
    refresh();
    const interval = setInterval(refresh, POLL_INTERVAL_MS);
    return () => clearInterval(interval);
  }, []);

  /** Pauses or resumes Gmail polling. */
  const toggleGmailPoll = async () => {
    if (!pipelineState) return;
    await API.debug.setGmailPollPaused(!pipelineState.gmail_poll_paused);
    refresh();
  };

  /** Pauses or resumes the scan queue. */
  const toggleScanQueue = async () => {
    if (!pipelineState) return;
    await API.debug.setScanQueuePaused(!pipelineState.scan_queue_paused);
    refresh();
  };

  return { metrics, pipelineState, ram, refresh, toggleGmailPoll, toggleScanQueue };
}
