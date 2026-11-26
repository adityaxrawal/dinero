// Startup LLM sync: the RAM guard is the only branch here that can lose a
// user's chosen model, so it gets pinned in all three directions.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { syncLlmState } from '@/lib/syncLlmState';
import { API } from '@/lib/ipc';
import { toast } from '@/hooks/use-toast';

vi.mock('@/hooks/use-toast', () => ({ toast: vi.fn() }));
vi.mock('@/lib/ipc', () => ({
  API: {
    llm: {
      getHardwareInfo: vi.fn(),
      getAvailableModels: vi.fn(),
      getActiveModel: vi.fn(),
      setActiveModel: vi.fn(),
      setParallelSlots: vi.fn(),
      setRamOverride: vi.fn(),
    },
  },
}));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;

const MODELS = [
  { id: 'small', name: 'Small', min_ram_gb: 8, approx_size_gb: 2, tier: 1, rationale: '' },
  { id: 'large', name: 'Large', min_ram_gb: 32, approx_size_gb: 20, tier: 5, rationale: '' },
];

const hw = (over = {}) =>
  ({
    ram_gb: 16,
    cpu_cores: 8,
    recommended_slots: 4,
    recommended_model_id: 'small',
    ram_override: false,
    ...over,
  }) as never;

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  asMock(API.llm.getAvailableModels).mockResolvedValue(MODELS);
  asMock(API.llm.getActiveModel).mockResolvedValue('');
  asMock(API.llm.setActiveModel).mockResolvedValue(undefined);
  asMock(API.llm.setParallelSlots).mockResolvedValue(undefined);
  asMock(API.llm.setRamOverride).mockResolvedValue(undefined);
});

/** The action button of the last toast, as rendered props. */
const toastAction = () =>
  (asMock(toast).mock.calls.at(-1)![0] as { action: { props: { onClick: () => void } } }).action
    .props;

describe('syncLlmState', () => {
  it('activates the hardware recommendation on a first run', async () => {
    asMock(API.llm.getHardwareInfo).mockResolvedValue(hw());
    await syncLlmState();
    expect(API.llm.setActiveModel).toHaveBeenCalledWith('small');
  });

  it('keeps a stored model this Mac can run', async () => {
    asMock(API.llm.getActiveModel).mockResolvedValue('large');
    asMock(API.llm.getHardwareInfo).mockResolvedValue(hw({ ram_gb: 64 }));
    await syncLlmState();
    expect(API.llm.setActiveModel).toHaveBeenCalledWith('large');
  });

  it('falls back to the lightest model on a Mac too small for the choice, without blocking (F7-5)', async () => {
    asMock(API.llm.getActiveModel).mockResolvedValue('large');
    asMock(API.llm.getHardwareInfo).mockResolvedValue(hw({ ram_gb: 16 }));
    await syncLlmState();
    expect(API.llm.setActiveModel).toHaveBeenCalledWith('small');
    expect(toast).toHaveBeenCalledWith(
      expect.objectContaining({ title: 'Not enough memory for this AI model' })
    );
    expect(API.llm.setRamOverride).not.toHaveBeenCalled();
  });

  it('"Use anyway" records the override in the backend and restores the choice', async () => {
    asMock(API.llm.getActiveModel).mockResolvedValue('large');
    asMock(API.llm.getHardwareInfo).mockResolvedValue(hw({ ram_gb: 16 }));
    await syncLlmState();
    toastAction().onClick();
    await vi.waitFor(() => expect(API.llm.setActiveModel).toHaveBeenLastCalledWith('large'));
    expect(API.llm.setRamOverride).toHaveBeenCalledWith(true);
    expect(localStorage.getItem('llm_ram_override')).toBeNull();
  });

  it('does not warn when the backend override is set', async () => {
    asMock(API.llm.getActiveModel).mockResolvedValue('large');
    asMock(API.llm.getHardwareInfo).mockResolvedValue(hw({ ram_gb: 16, ram_override: true }));
    await syncLlmState();
    expect(toast).not.toHaveBeenCalled();
    expect(API.llm.setActiveModel).toHaveBeenCalledWith('large');
  });

  it('migrates a localStorage override to the backend once, then forgets it', async () => {
    localStorage.setItem('llm_ram_override', 'true');
    asMock(API.llm.getHardwareInfo).mockResolvedValue(hw());
    await syncLlmState();
    expect(API.llm.setRamOverride).toHaveBeenCalledWith(true);
    expect(localStorage.getItem('llm_ram_override')).toBeNull();
    vi.clearAllMocks();
    await syncLlmState();
    expect(API.llm.setRamOverride).not.toHaveBeenCalled();
  });

  it('the database preference wins over the hardware recommendation (audit B-3)', async () => {
    // A portable backup restored onto a new Mac carries its model choice in
    // the database; localStorage on the new Mac is empty.
    asMock(API.llm.getActiveModel).mockResolvedValue('large');
    asMock(API.llm.getHardwareInfo).mockResolvedValue(
      hw({ ram_gb: 64, recommended_model_id: 'small' })
    );
    await syncLlmState();
    expect(API.llm.setActiveModel).toHaveBeenCalledWith('large');
    expect(localStorage.getItem('llm_model')).toBeNull();
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
