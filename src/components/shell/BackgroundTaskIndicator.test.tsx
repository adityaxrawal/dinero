import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import BackgroundTaskIndicator from './BackgroundTaskIndicator';
import { API } from '@/lib/ipc';

vi.mock('@/lib/ipc', async () => {
  const actual = await vi.importActual<typeof import('@/lib/ipc')>('@/lib/ipc');
  return {
    ...actual,
    API: {
      backgroundTasks: {
        getActive: vi.fn(),
      },
    },
  };
});

let ipcListenHandler: ((payload: any) => void) | null = null;
vi.mock('@/hooks/useIpcListen', () => ({
  useIpcListen: (_event: string, handler: (payload: any) => void) => {
    ipcListenHandler = handler;
  },
}));

const runningTask = {
  task_id: 'scan_1',
  task_type: 'llm_download',
  label: 'Scanning acct_1',
  current: 10,
  total: 100,
  eta_seconds: 30,
  status: 'running' as const,
  progress_pct: 10,
  status_message: 'Scanning...',
};

const failedTask = {
  ...runningTask,
  status: 'failed' as const,
  status_message: 'Gmail API rate limited after 3 retries',
};

describe('BackgroundTaskIndicator', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    ipcListenHandler = null;
  });

  it('renders nothing when there are no active tasks', async () => {
    (API.backgroundTasks.getActive as any).mockResolvedValue([]);
    render(<BackgroundTaskIndicator />);
    await waitFor(() => expect(API.backgroundTasks.getActive).toHaveBeenCalled());
    expect(screen.queryByTestId('bg-task-indicator')).toBeNull();
  });

  it('test_indicator_shows_aggregate_when_multiple_tasks_active', async () => {
    (API.backgroundTasks.getActive as any).mockResolvedValue([
      runningTask,
      { ...runningTask, task_id: 'scan_2', label: 'Scanning acct_2' },
    ]);
    render(<BackgroundTaskIndicator />);
    await waitFor(() => {
      expect(screen.getByText('2 tasks running')).toBeTruthy();
    });
  });

  it('recovers an already-running task via late-mount getActive fetch', async () => {
    (API.backgroundTasks.getActive as any).mockResolvedValue([runningTask]);
    render(<BackgroundTaskIndicator />);
    await waitFor(() => {
      expect(screen.getByTestId('bg-task-indicator')).toBeTruthy();
      expect(screen.getByText('Scanning acct_1')).toBeTruthy();
    });
  });

  it('test_failed_task_shows_distinct_error_chip: a failed task persists with a View Details action, not silently disappearing', async () => {
    (API.backgroundTasks.getActive as any).mockResolvedValue([runningTask]);
    render(<BackgroundTaskIndicator />);
    await waitFor(() => expect(ipcListenHandler).toBeDefined());

    ipcListenHandler!(failedTask);

    await waitFor(() => {
      expect(screen.getByTestId(`failed-task-chip-${failedTask.task_id}`)).toBeTruthy();
    });
    expect(screen.getByText('View Details')).toBeTruthy();
    expect(screen.queryByText(failedTask.status_message)).toBeNull();

    fireEvent.click(screen.getByText('View Details'));
    expect(screen.getByText(failedTask.status_message)).toBeTruthy();
  });

  it('a completed task is removed automatically, unlike a failed one', async () => {
    (API.backgroundTasks.getActive as any).mockResolvedValue([runningTask]);
    render(<BackgroundTaskIndicator />);
    await waitFor(() => expect(ipcListenHandler).toBeDefined());

    ipcListenHandler!({ ...runningTask, status: 'completed' });

    await waitFor(() => {
      expect(screen.queryByTestId('bg-task-indicator')).toBeNull();
    });
  });

  it('dismissing a failed task removes its chip', async () => {
    (API.backgroundTasks.getActive as any).mockResolvedValue([failedTask]);
    render(<BackgroundTaskIndicator />);
    await waitFor(() => {
      expect(screen.getByTestId(`failed-task-chip-${failedTask.task_id}`)).toBeTruthy();
    });

    fireEvent.click(screen.getByLabelText('Dismiss failed task'));
    await waitFor(() => {
      expect(screen.queryByTestId('bg-task-indicator')).toBeNull();
    });
  });

  it('test_historical_scan_tasks_are_filtered_out: they get their own sidebar status readout instead', async () => {
    (API.backgroundTasks.getActive as any).mockResolvedValue([
      { ...runningTask, task_id: 'scan_1', task_type: 'historical_scan' },
    ]);
    render(<BackgroundTaskIndicator />);
    await waitFor(() => expect(API.backgroundTasks.getActive).toHaveBeenCalled());
    expect(screen.queryByTestId('bg-task-indicator')).toBeNull();
  });

  it('a historical_scan task does not count toward the multi-task aggregate', async () => {
    (API.backgroundTasks.getActive as any).mockResolvedValue([
      runningTask,
      { ...runningTask, task_id: 'scan_2', task_type: 'historical_scan' },
    ]);
    render(<BackgroundTaskIndicator />);
    await waitFor(() => {
      // Only runningTask (llm_download) counts -- a single task renders its
      // own progress bar rather than "N tasks running".
      expect(screen.getByTestId('bg-task-indicator')).toBeTruthy();
      expect(screen.queryByText(/tasks running/)).toBeNull();
    });
  });
});
