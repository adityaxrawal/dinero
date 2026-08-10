// Covers the pipeline pause/resume controls extracted out of Debug.tsx.
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import PipelineSection from '@/pages/debug/PipelineSection';
import type { useDebugMetrics } from '@/pages/debug/useDebugMetrics';

const toggleGmailPoll = vi.fn();
const toggleScanQueue = vi.fn();

const debug = (gmailPaused: boolean, scanPaused: boolean) =>
  ({
    metrics: null,
    ram: null,
    refresh: vi.fn(),
    pipelineState: { gmail_poll_paused: gmailPaused, scan_queue_paused: scanPaused },
    toggleGmailPoll,
    toggleScanQueue,
  }) as unknown as ReturnType<typeof useDebugMetrics>;

describe('PipelineSection', () => {
  it('shows both pipelines running and offers to pause each', () => {
    render(<PipelineSection debug={debug(false, false)} />);
    expect(screen.getAllByText('RUNNING')).toHaveLength(2);
    expect(screen.getByRole('button', { name: /Pause Polling/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Pause Scan/ })).toBeInTheDocument();
  });

  it('flips to resume once a pipeline is paused', () => {
    render(<PipelineSection debug={debug(true, false)} />);
    expect(screen.getByText('PAUSED')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Resume Polling/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Pause Scan/ })).toBeInTheDocument();
  });

  it('toggles each pipeline independently', () => {
    render(<PipelineSection debug={debug(false, false)} />);
    fireEvent.click(screen.getByRole('button', { name: /Pause Polling/ }));
    expect(toggleGmailPoll).toHaveBeenCalled();
    expect(toggleScanQueue).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: /Pause Scan/ }));
    expect(toggleScanQueue).toHaveBeenCalled();
  });

  it('treats an unknown pipeline state as running rather than blank', () => {
    render(
      <PipelineSection
        debug={{ pipelineState: null } as unknown as ReturnType<typeof useDebugMetrics>}
      />
    );
    expect(screen.getAllByText('RUNNING')).toHaveLength(2);
  });
});
