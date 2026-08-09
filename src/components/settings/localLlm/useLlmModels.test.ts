// Activating a model heavier than this Mac can host is the one action here
// that can make the app unusable, so the RAM guard is pinned in all branches.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { useLlmModels } from './useLlmModels';
import type { LlmModelInfo } from '@/lib/ipc';
import { API } from '@/lib/ipc';

vi.mock('@/hooks/useIpcListen', () => ({ useIpcListen: vi.fn() }));
vi.mock('@/lib/ipc', () => ({
  API: {
    llm: {
      getAvailableModels: vi.fn(),
      getDownloadedModels: vi.fn(),
      getActiveModel: vi.fn(),
      getHardwareInfo: vi.fn(),
      setParallelSlots: vi.fn(),
      setActiveModel: vi.fn(),
      downloadModel: vi.fn(),
      cancelDownload: vi.fn(),
      deleteModel: vi.fn(),
    },
    dev: { checkSystemRam: vi.fn() },
  },
}));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;

const HEAVY = {
  id: 'large',
  name: 'Large',
  min_ram_gb: 32,
  approx_size_gb: 20,
  tier: 5,
  rationale: '',
  tag: 'large',
  gguf_url: 'https://example.test/large.gguf',
  expected_sha256: 'abc',
  tokenizer_url: 'https://example.test/tok.json',
} as LlmModelInfo;
const onHardware = vi.fn(() => 4);

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  asMock(API.llm.getAvailableModels).mockResolvedValue([HEAVY]);
  asMock(API.llm.getDownloadedModels).mockResolvedValue(['large']);
  asMock(API.llm.getActiveModel).mockResolvedValue('');
  asMock(API.llm.getHardwareInfo).mockResolvedValue({
    ram_gb: 16,
    cpu_cores: 8,
    recommended_slots: 4,
    recommended_model_id: 'large',
  });
  asMock(API.llm.setParallelSlots).mockResolvedValue(undefined);
  asMock(API.llm.setActiveModel).mockResolvedValue(undefined);
  asMock(API.dev.checkSystemRam).mockResolvedValue(16);
  vi.stubGlobal('alert', vi.fn());
  vi.stubGlobal('confirm', vi.fn(() => false));
});

async function setup() {
  const hook = renderHook(() => useLlmModels(onHardware));
  await waitFor(() => expect(hook.result.current.availableModels.length).toBe(1));
  return hook;
}

describe('useLlmModels.setActive', () => {
  it('refuses a model that has not been downloaded', async () => {
    asMock(API.llm.getDownloadedModels).mockResolvedValue([]);
    const { result } = await setup();
    await act(async () => {
      await result.current.setActive(HEAVY);
    });
    expect(alert).toHaveBeenCalledWith('You need to download this model first.');
    expect(API.llm.setActiveModel).not.toHaveBeenCalled();
  });

  it('activates without warning when there is enough RAM', async () => {
    asMock(API.dev.checkSystemRam).mockResolvedValue(64);
    const { result } = await setup();
    await act(async () => {
      await result.current.setActive(HEAVY);
    });
    expect(confirm).not.toHaveBeenCalled();
    expect(API.llm.setActiveModel).toHaveBeenCalledWith('large');
  });

  it('abandons activation when the user declines the RAM warning', async () => {
    const { result } = await setup();
    await act(async () => {
      await result.current.setActive(HEAVY);
    });
    expect(confirm).toHaveBeenCalled();
    expect(API.llm.setActiveModel).not.toHaveBeenCalled();
  });

  it('activates and records an override when the user accepts', async () => {
    vi.stubGlobal('confirm', vi.fn(() => true));
    const { result } = await setup();
    await act(async () => {
      await result.current.setActive(HEAVY);
    });
    expect(API.llm.setActiveModel).toHaveBeenCalledWith('large');
    expect(localStorage.getItem('llm_ram_override')).toBe('true');
  });

  it('skips the warning entirely once an override is stored', async () => {
    localStorage.setItem('llm_ram_override', 'true');
    const { result } = await setup();
    await act(async () => {
      await result.current.setActive(HEAVY);
    });
    expect(confirm).not.toHaveBeenCalled();
    expect(API.llm.setActiveModel).toHaveBeenCalledWith('large');
  });

  it('does not block the user when the RAM probe itself fails', async () => {
    asMock(API.dev.checkSystemRam).mockRejectedValue(new Error('no sysinfo'));
    vi.spyOn(console, 'error').mockImplementation(() => {});
    const { result } = await setup();
    await act(async () => {
      await result.current.setActive(HEAVY);
    });
    expect(API.llm.setActiveModel).toHaveBeenCalledWith('large');
  });

  it('reports a failed activation instead of silently doing nothing', async () => {
    asMock(API.dev.checkSystemRam).mockResolvedValue(64);
    asMock(API.llm.setActiveModel).mockRejectedValue(new Error('sidecar down'));
    const { result } = await setup();
    await act(async () => {
      await result.current.setActive(HEAVY);
    });
    expect(alert).toHaveBeenCalledWith(expect.stringContaining('Failed to set active model'));
  });
});
