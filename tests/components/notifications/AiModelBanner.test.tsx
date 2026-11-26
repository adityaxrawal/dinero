// Fallback when onboarding's model download was skipped or failed (Q-1).
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import AiModelBanner from '@/components/notifications/AiModelBanner';

let queue: { pending: number; state: string } | undefined;
let progress: unknown = null;
const start = vi.fn();

vi.mock('@/hooks/queries/useLlmQueueStatus', () => ({ useLlmQueueStatus: () => ({ data: queue }) }));
vi.mock('@/hooks/useModelDownload', () => ({
  useModelDownload: () => ({ progress, start }),
  downloadPercent: () => 40,
}));
vi.mock('@/hooks/use-toast', () => ({ useToast: () => ({ toast: vi.fn() }) }));

beforeEach(() => {
  vi.clearAllMocks();
  progress = null;
  start.mockResolvedValue(undefined);
});

describe('AiModelBanner', () => {
  it('offers a download when no model is installed', async () => {
    queue = { pending: 50, state: 'no_model' };
    render(<AiModelBanner />);
    expect(screen.getByRole('status').textContent).toContain('50 emails are waiting');
    fireEvent.click(screen.getByRole('button', { name: 'Download' }));
    await waitFor(() => expect(start).toHaveBeenCalled());
  });

  it('shows progress while downloading', () => {
    queue = { pending: 0, state: 'no_model' };
    progress = { model_id: 'm', bytes_downloaded: 40, total_bytes: 100, bytes_per_sec: 1 };
    render(<AiModelBanner />);
    expect(screen.getByRole('status').textContent).toContain('Downloading the AI model… 40%');
  });

  it('stays hidden when the AI is ready', () => {
    queue = { pending: 3, state: 'ready' };
    const { container } = render(<AiModelBanner />);
    expect(container.firstChild).toBeNull();
  });
});
