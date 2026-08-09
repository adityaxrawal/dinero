import { API, type LlmHardwareInfo, type LlmModelInfo } from '@/lib/ipc';
import {
  PARALLEL_SLOTS_STORAGE_KEY,
  RAM_OVERRIDE_STORAGE_KEY,
  clampSlots,
  storedSlots,
} from '@/components/settings/localLlm/format';

const MODEL_STORAGE_KEY = 'llm_model';

/** The lowest-tier model, i.e. the broadest-compatibility fallback. */
function lightestModel(models: LlmModelInfo[]): LlmModelInfo {
  return models.reduce((a, b) => (a.min_ram_gb <= b.min_ram_gb ? a : b));
}

/**
 * Returns the model id to activate. When the stored choice needs more RAM than
 * this Mac has, the user either accepts the risk (recorded as a persistent
 * override) or is silently moved to the lightest model.
 */
function resolveModelId(hw: LlmHardwareInfo, models: LlmModelInfo[]): string {
  const selectedId = localStorage.getItem(MODEL_STORAGE_KEY) || hw.recommended_model_id || models[0].id;
  const selected = models.find((m) => m.id === selectedId);
  const overridden = localStorage.getItem(RAM_OVERRIDE_STORAGE_KEY) === 'true';

  if (!selected || overridden || hw.ram_gb >= selected.min_ram_gb) {
    // Ensure localStorage is populated if it was empty.
    localStorage.setItem(MODEL_STORAGE_KEY, selectedId);
    return selectedId;
  }

  const accepted = window.confirm(
    `Warning: Your system has ${hw.ram_gb.toFixed(1)}GB of RAM, but ${selected.name} requires at least ${selected.min_ram_gb}GB for optimal performance. You may experience slow downs or crashes.\n\nDo you want to continue anyway (allow override)?`
  );
  if (accepted) {
    localStorage.setItem(RAM_OVERRIDE_STORAGE_KEY, 'true');
    return selectedId;
  }

  const fallback = lightestModel(models);
  localStorage.setItem(MODEL_STORAGE_KEY, fallback.id);
  alert(
    `Model automatically switched to a lighter version (${fallback.name}). You can change this in Settings.`
  );
  return fallback.id;
}

/**
 * Doc 16 §12.3: the 5-tier model catalog is the single source of truth for RAM
 * requirements — never a hardcoded model id/threshold here.
 */
export async function syncLlmState(): Promise<void> {
  try {
    const [hw, models] = await Promise.all([
      API.llm.getHardwareInfo(),
      API.llm.getAvailableModels(),
    ]);
    if (models.length === 0) return;

    await API.llm.setActiveModel(resolveModelId(hw, models));

    const initialSlots = storedSlots() ?? clampSlots(hw.recommended_slots);
    await API.llm.setParallelSlots(initialSlots);
    localStorage.setItem(PARALLEL_SLOTS_STORAGE_KEY, initialSlots.toString());
  } catch (e) {
    console.error('Failed to sync LLM state on startup', e);
  }
}
