// Startup LLM sync: the RAM guard is the only branch here that can lose a
// user's chosen model, so it gets pinned in all three directions.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { syncLlmState } from '@/lib/syncLlmState';
import { API } from '@/lib/ipc';

vi.mock('@/lib/ipc', () => ({
  API: {
    llm: {
      getHardwareInfo: vi.fn(),
      getAvailableModels: vi.fn(),
      setActiveModel: vi.fn(),
      setParallelSlots: vi.fn(),
    },
  },
}));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;

const MODELS = [
  { id: 'small', name: 'Small', min_ram_gb: 8, approx_size_gb: 2, tier: 1, rationale: '' },
  { id: 'large', name: 'Large', min_ram_gb: 32, approx_size_gb: 20, tier: 5, rationale: '' },
];

const hw = (over = {}) =>
  ({ ram_gb: 16, cpu_cores: 8, recommended_slots: 4, recommended_model_id: 'small', ...over }) as never;

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  asMock(API.llm.getAvailableModels).mockResolvedValue(MODELS);
  asMock(API.llm.setActiveModel).mockResolvedValue(undefined);
  asMock(API.llm.setParallelSlots).mockResolvedValue(undefined);
  vi.stubGlobal('confirm', vi.fn(() => false));
  vi.stubGlobal('alert', vi.fn());
});

describe('syncLlmState', () => {
  it('activates the hardware recommendation on a first run', async () => {
    asMock(API.llm.getHardwareInfo).mockResolvedValue(hw());
    await syncLlmState();
    expect(API.llm.setActiveModel).toHaveBeenCalledWith('small');
    expect(localStorage.getItem('llm_model')).toBe('small');
  });

  it('keeps a stored model this Mac can run', async () => {
    localStorage.setItem('llm_model', 'large');
    asMock(API.llm.getHardwareInfo).mockResolvedValue(hw({ ram_gb: 64 }));
    await syncLlmState();
    expect(API.llm.setActiveModel).toHaveBeenCalledWith('large');
  });

  it('falls back to the lightest model when the user declines the RAM warning', async () => {
    localStorage.setItem('llm_model', 'large');
    asMock(API.llm.getHardwareInfo).mockResolvedValue(hw({ ram_gb: 16 }));
    await syncLlmState();
    expect(API.llm.setActiveModel).toHaveBeenCalledWith('small');
    expect(localStorage.getItem('llm_model')).toBe('small');
    expect(alert).toHaveBeenCalled();
  });

  it('records a persistent override when the user accepts the risk', async () => {
    localStorage.setItem('llm_model', 'large');
    vi.stubGlobal('confirm', vi.fn(() => true));
    asMock(API.llm.getHardwareInfo).mockResolvedValue(hw({ ram_gb: 16 }));
    await syncLlmState();
    expect(API.llm.setActiveModel).toHaveBeenCalledWith('large');
    expect(localStorage.getItem('llm_ram_override')).toBe('true');
  });

  it('does not warn again once the override is stored', async () => {
    localStorage.setItem('llm_model', 'large');
    localStorage.setItem('llm_ram_override', 'true');
    asMock(API.llm.getHardwareInfo).mockResolvedValue(hw({ ram_gb: 16 }));
    await syncLlmState();
    expect(confirm).not.toHaveBeenCalled();
    expect(API.llm.setActiveModel).toHaveBeenCalledWith('large');
  });

  it('adopts the recommended slot count, then persists it', async () => {
    asMock(API.llm.getHardwareInfo).mockResolvedValue(hw({ recommended_slots: 6 }));
    await syncLlmState();
    expect(API.llm.setParallelSlots).toHaveBeenCalledWith(6);
    expect(localStorage.getItem('llm_parallel_slots')).toBe('6');
  });

  it('clamps a stored slot count into the 1-10 range', async () => {
    localStorage.setItem('llm_parallel_slots', '99');
    asMock(API.llm.getHardwareInfo).mockResolvedValue(hw());
    await syncLlmState();
    expect(API.llm.setParallelSlots).toHaveBeenCalledWith(10);
  });

  it('does nothing at all when the catalog comes back empty', async () => {
    asMock(API.llm.getHardwareInfo).mockResolvedValue(hw());
    asMock(API.llm.getAvailableModels).mockResolvedValue([]);
    await syncLlmState();
    expect(API.llm.setActiveModel).not.toHaveBeenCalled();
  });

  it('never lets a startup failure escape', async () => {
    asMock(API.llm.getHardwareInfo).mockRejectedValue(new Error('sidecar down'));
    await expect(syncLlmState()).resolves.toBeUndefined();
  });
});
