import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react';
import LocalLlmSettings from './LocalLlmSettings';
import { API, type LlmModelInfo } from '@/lib/ipc';

vi.mock('@/lib/ipc', () => ({
  API: {
    llm: {
      getAvailableModels: vi.fn(),
      getDownloadedModels: vi.fn(),
      getActiveModel: vi.fn(),
      downloadModel: vi.fn(),
      deleteModel: vi.fn(),
      cancelDownload: vi.fn(),
      setActiveModel: vi.fn(),
      getHardwareInfo: vi.fn(),
      setParallelSlots: vi.fn(),
    },
    dev: {
      checkSystemRam: vi.fn().mockResolvedValue(64),
    },
  },
}));

let ipcListenHandler: ((payload: unknown) => void) | null = null;
vi.mock('@/hooks/useIpcListen', () => ({
  useIpcListen: (_event: string, handler: (payload: unknown) => void) => {
    ipcListenHandler = handler;
  },
}));

const MODELS: LlmModelInfo[] = [
  {
    id: 'gemma4_e4b',
    name: 'Gemma 4 E4B',
    tag: 'gemma4:e4b',
    tier: 1,
    min_ram_gb: 8,
    approx_size_gb: 5,
    rationale: 'r1',
    gguf_url: 'u1',
    expected_sha256: 'h1',
    tokenizer_url: null,
  },
  {
    id: 'gemma4_12b',
    name: 'Gemma 4 12B',
    tag: 'gemma4:12b',
    tier: 2,
    min_ram_gb: 16,
    approx_size_gb: 9,
    rationale: 'r2',
    gguf_url: 'u2',
    expected_sha256: 'h2',
    tokenizer_url: null,
  },
];

describe('LocalLlmSettings', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    ipcListenHandler = null;
    localStorage.clear();
    // Default hardware info for tests that don't care about it — the four
    // pre-existing tests below just need the load Promise.all to resolve.
    vi.mocked(API.llm.getHardwareInfo).mockResolvedValue({
      ram_gb: 64,
      cpu_cores: 8,
      recommended_slots: 4,
      recommended_model_id: null,
    });
    vi.mocked(API.llm.setParallelSlots).mockResolvedValue(4);
  });

  it('reassigns the Active badge to the remaining downloaded model when the active model is deleted', async () => {
    vi.mocked(API.llm.getAvailableModels).mockResolvedValue(MODELS);
    vi.mocked(API.llm.getDownloadedModels).mockResolvedValueOnce(['gemma4_e4b', 'gemma4_12b']) // initial load
      .mockResolvedValueOnce(['gemma4_e4b']); // after delete
    vi.mocked(API.llm.getActiveModel).mockResolvedValue('gemma4_12b');
    // Backend self-heals and returns the new active model id directly.
    vi.mocked(API.llm.deleteModel).mockResolvedValue('gemma4_e4b');
    vi.spyOn(window, 'confirm').mockReturnValue(true);

    render(<LocalLlmSettings />);

    await waitFor(() => {
      expect(screen.getAllByText('Active')).toHaveLength(1);
    });
    const e4bCardHeading = screen.getByText('Gemma 4 E4B');
    const e4bCard = e4bCardHeading.closest('div.p-5') as HTMLElement;
    expect(e4bCard).not.toBeNull();
    expect(within(e4bCard).queryByText('Active')).toBeNull();

    const twelveBCardHeading = screen.getByText('Gemma 4 12B');
    const twelveBCard = twelveBCardHeading.closest('div.p-5') as HTMLElement;
    const deleteButton = within(twelveBCard).getByTitle('Delete model');
    fireEvent.click(deleteButton);

    await waitFor(() => {
      expect(API.llm.deleteModel).toHaveBeenCalledWith('gemma4_12b');
    });

    await waitFor(() => {
      const activeBadges = screen.getAllByText('Active');
      expect(activeBadges).toHaveLength(1);
    });
    const e4bCardAfter = screen.getByText('Gemma 4 E4B').closest('div.p-5') as HTMLElement;
    expect(within(e4bCardAfter).getByText('Active')).toBeInTheDocument();
    const twelveBCardAfter = screen.getByText('Gemma 4 12B').closest('div.p-5') as HTMLElement;
    expect(within(twelveBCardAfter).queryByText('Active')).toBeNull();
  });

  it('renders no Active badge when the only downloaded model is deleted', async () => {
    vi.mocked(API.llm.getAvailableModels).mockResolvedValue(MODELS);
    vi.mocked(API.llm.getDownloadedModels).mockResolvedValueOnce(['gemma4_12b'])
      .mockResolvedValueOnce([]);
    vi.mocked(API.llm.getActiveModel).mockResolvedValue('gemma4_12b');
    vi.mocked(API.llm.deleteModel).mockResolvedValue('');
    vi.spyOn(window, 'confirm').mockReturnValue(true);

    render(<LocalLlmSettings />);

    await waitFor(() => {
      expect(screen.getAllByText('Active')).toHaveLength(1);
    });

    const twelveBCard = screen.getByText('Gemma 4 12B').closest('div.p-5') as HTMLElement;
    fireEvent.click(within(twelveBCard).getByTitle('Delete model'));

    await waitFor(() => {
      expect(API.llm.deleteModel).toHaveBeenCalledWith('gemma4_12b');
    });
    await waitFor(() => {
      expect(screen.queryByText('Active')).toBeNull();
    });
  });

  it('shows live download speed and ETA from progress events', async () => {
    vi.mocked(API.llm.getAvailableModels).mockResolvedValue(MODELS);
    vi.mocked(API.llm.getDownloadedModels).mockResolvedValue([]);
    vi.mocked(API.llm.getActiveModel).mockResolvedValue('');
    // Never resolves — mirrors the real command, which only resolves once
    // the whole multi-GB download finishes. Keeps the component in the
    // "downloading" state so the progress event we dispatch below is
    // actually rendered instead of the download flow completing first.
    vi.mocked(API.llm.downloadModel).mockReturnValue(new Promise(() => {}));

    render(<LocalLlmSettings />);

    await waitFor(() => expect(screen.getByText('Gemma 4 E4B')).toBeInTheDocument());
    expect(ipcListenHandler).not.toBeNull();

    const downloadButtons = screen.getAllByRole('button', { name: /download/i });
    fireEvent.click(downloadButtons[0]);

    // Simulate a Rust-side llm_download_progress event via the captured handler.
    ipcListenHandler!({
      model_id: 'gemma4_e4b',
      bytes_downloaded: 1_048_576 * 500, // 500 MiB
      total_bytes: 1_048_576 * 5000, // 5000 MiB
      bytes_per_sec: 1_048_576 * 5, // 5 MB/s
    });

    await waitFor(() => {
      expect(screen.getByText(/5\.0 MB\/s/)).toBeInTheDocument();
    });
    expect(screen.getByText(/left/)).toBeInTheDocument();
    expect(screen.getByText('10%')).toBeInTheDocument();
  });

  it('cancels an in-progress download via the Cancel button', async () => {
    vi.mocked(API.llm.getAvailableModels).mockResolvedValue(MODELS);
    vi.mocked(API.llm.getDownloadedModels).mockResolvedValueOnce([]) // initial load
      .mockResolvedValueOnce([]); // after the cancelled download's cleanup fetch
    vi.mocked(API.llm.getActiveModel).mockResolvedValue('');

    let resolveDownload: (() => void) | undefined;
    vi.mocked(API.llm.downloadModel).mockReturnValue(
      new Promise<void>((resolve) => {
        resolveDownload = resolve;
      })
    );
    vi.mocked(API.llm.cancelDownload).mockImplementation(async () => {
      resolveDownload?.();
    });

    render(<LocalLlmSettings />);

    await waitFor(() => expect(screen.getByText('Gemma 4 E4B')).toBeInTheDocument());
    fireEvent.click(screen.getAllByRole('button', { name: /download/i })[0]);

    await waitFor(() => {
      expect(screen.getByTitle('Cancel download')).toBeInTheDocument();
    });
    fireEvent.click(screen.getByTitle('Cancel download'));

    await waitFor(() => {
      expect(API.llm.cancelDownload).toHaveBeenCalledWith('gemma4_e4b');
    });
    await waitFor(() => {
      expect(screen.queryByTitle('Cancel download')).toBeNull();
    });
  });

  it('shows recommended parallel instances and a hardware-recommended model badge', async () => {
    vi.mocked(API.llm.getAvailableModels).mockResolvedValue(MODELS);
    vi.mocked(API.llm.getDownloadedModels).mockResolvedValue(['gemma4_e4b']);
    vi.mocked(API.llm.getActiveModel).mockResolvedValue('gemma4_e4b');
    vi.mocked(API.llm.getHardwareInfo).mockResolvedValue({
      ram_gb: 24,
      cpu_cores: 10,
      recommended_slots: 5,
      recommended_model_id: 'gemma4_12b',
    });
    vi.mocked(API.llm.setParallelSlots).mockResolvedValue(5);

    render(<LocalLlmSettings />);

    await waitFor(() => {
      expect(screen.getByDisplayValue('5')).toBeInTheDocument();
    });
    // "Recommended: " and the "5" are separate text nodes (the count is
    // wrapped in <strong>), so a plain string/regex match against a single
    // node won't find it — match on the element's full merged textContent
    // instead, scoped to the deepest element that contains it.
    expect(
      screen.getByText((_content, node) => {
        const text = node?.textContent?.replace(/\s+/g, ' ') ?? '';
        const hasIt = text.includes('Recommended: 5');
        const childHasIt = Array.from(node?.children ?? []).some((child) =>
          (child.textContent?.replace(/\s+/g, ' ') ?? '').includes('Recommended: 5')
        );
        return hasIt && !childHasIt;
      })
    ).toBeInTheDocument();

    const twelveBCard = screen.getByText('Gemma 4 12B').closest('div.p-5') as HTMLElement;
    expect(within(twelveBCard).getByText('Recommended for your Mac')).toBeInTheDocument();

    await waitFor(() => {
      expect(API.llm.setParallelSlots).toHaveBeenCalledWith(5);
    });
  });

  it('clamps a user-entered instance count to 1-10 but waits for Save to persist it', async () => {
    vi.mocked(API.llm.getAvailableModels).mockResolvedValue(MODELS);
    vi.mocked(API.llm.getDownloadedModels).mockResolvedValue([]);
    vi.mocked(API.llm.getActiveModel).mockResolvedValue('');
    vi.mocked(API.llm.getHardwareInfo).mockResolvedValue({
      ram_gb: 8,
      cpu_cores: 4,
      recommended_slots: 1,
      recommended_model_id: 'gemma4_e4b',
    });
    vi.mocked(API.llm.setParallelSlots).mockResolvedValue(1);

    render(<LocalLlmSettings />);

    const input = await screen.findByLabelText('Number of parallel LLM instances');
    await waitFor(() => expect(API.llm.setParallelSlots).toHaveBeenCalledWith(1)); // initial load sync

    fireEvent.change(input, { target: { value: '15' } });
    expect(input).toHaveValue(10); // clamped immediately in the input

    // Typing alone must not push a new value or restart the server. Nothing
    // has been explicitly saved yet, so localStorage stays untouched (the
    // initial-load sync pushes to the backend only, never to localStorage).
    expect(API.llm.setParallelSlots).not.toHaveBeenCalledWith(10);
    expect(localStorage.getItem('llm_parallel_slots')).toBeNull();

    const saveButton = screen.getByRole('button', { name: /save/i });
    expect(saveButton).toBeEnabled();
    fireEvent.click(saveButton);

    await waitFor(() => {
      expect(API.llm.setParallelSlots).toHaveBeenCalledWith(10);
    });
    expect(localStorage.getItem('llm_parallel_slots')).toBe('10');
  });

  it('shows an above-recommended-tier warning when the selected model is heavier than recommended', async () => {
    vi.mocked(API.llm.getAvailableModels).mockResolvedValue(MODELS);
    vi.mocked(API.llm.getDownloadedModels).mockResolvedValue(['gemma4_e4b', 'gemma4_12b']);
    vi.mocked(API.llm.getActiveModel).mockResolvedValue('gemma4_12b');
    vi.mocked(API.llm.getHardwareInfo).mockResolvedValue({
      ram_gb: 8,
      cpu_cores: 4,
      recommended_slots: 1,
      recommended_model_id: 'gemma4_e4b',
    });
    vi.mocked(API.llm.setParallelSlots).mockResolvedValue(1);

    render(<LocalLlmSettings />);

    await waitFor(() => {
      expect(screen.getByText(/heavier than recommended/i)).toBeInTheDocument();
    });
    expect(screen.getAllByText(/Gemma 4 12B/).length).toBeGreaterThan(0);
  });

  it('shows no warning when the selected model is at or below the recommended tier', async () => {
    vi.mocked(API.llm.getAvailableModels).mockResolvedValue(MODELS);
    vi.mocked(API.llm.getDownloadedModels).mockResolvedValue(['gemma4_e4b']);
    vi.mocked(API.llm.getActiveModel).mockResolvedValue('gemma4_e4b');
    vi.mocked(API.llm.getHardwareInfo).mockResolvedValue({
      ram_gb: 8,
      cpu_cores: 4,
      recommended_slots: 1,
      recommended_model_id: 'gemma4_e4b',
    });
    vi.mocked(API.llm.setParallelSlots).mockResolvedValue(1);

    render(<LocalLlmSettings />);

    await waitFor(() => expect(screen.getByText('Gemma 4 E4B')).toBeInTheDocument());
    expect(screen.queryByText(/heavier than recommended/i)).toBeNull();
  });

  it('disables Save until the instance count is actually changed', async () => {
    vi.mocked(API.llm.getAvailableModels).mockResolvedValue(MODELS);
    vi.mocked(API.llm.getDownloadedModels).mockResolvedValue([]);
    vi.mocked(API.llm.getActiveModel).mockResolvedValue('');
    vi.mocked(API.llm.getHardwareInfo).mockResolvedValue({
      ram_gb: 8,
      cpu_cores: 4,
      recommended_slots: 1,
      recommended_model_id: 'gemma4_e4b',
    });
    vi.mocked(API.llm.setParallelSlots).mockResolvedValue(1);

    render(<LocalLlmSettings />);
    await screen.findByLabelText('Number of parallel LLM instances');

    const saveButton = screen.getByRole('button', { name: /save/i });
    expect(saveButton).toBeDisabled();
  });
});
