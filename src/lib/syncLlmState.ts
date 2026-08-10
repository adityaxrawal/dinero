/**
 * Reconciles the user's saved local-LLM preferences with the machine's actual
 * hardware at application startup.
 *
 * The stored model choice is a preference, not a guarantee: the same profile can
 * follow a user onto a machine with less RAM, and a model that ran acceptably
 * before may now thrash or crash the process. Rather than failing at inference
 * time, this module validates the choice up front and, when the machine cannot
 * support it, gives the user an explicit decision to make.
 *
 * The resolved model and slot count are pushed into the Rust backend, which owns
 * the actual inference runtime; localStorage holds only the preference.
 */
import { API, type LlmHardwareInfo, type LlmModelInfo } from '@/lib/ipc';
import {
  PARALLEL_SLOTS_STORAGE_KEY,
  RAM_OVERRIDE_STORAGE_KEY,
  clampSlots,
  storedSlots,
} from '@/components/settings/localLlm/format';

const MODEL_STORAGE_KEY = 'llm_model';

/** The model with the smallest RAM requirement -- the safe fallback for any machine. */
function lightestModel(models: LlmModelInfo[]): LlmModelInfo {
  return models.reduce((a, b) => (a.min_ram_gb <= b.min_ram_gb ? a : b));
}

/**
 * Decide which model to activate, prompting the user if their choice exceeds
 * what this machine can comfortably run.
 *
 * Note this deliberately blocks on native `confirm`/`alert` during startup. The
 * decision determines what the backend loads, so it must be settled before the
 * app proceeds rather than surfaced asynchronously afterwards.
 */
function resolveModelId(hw: LlmHardwareInfo, models: LlmModelInfo[]): string {
  // Preference order: what the user last chose, else the backend's hardware-based
  // recommendation, else the first available model.
  const selectedId = localStorage.getItem(MODEL_STORAGE_KEY) || hw.recommended_model_id || models[0].id;
  const selected = models.find((m) => m.id === selectedId);

  // A previously accepted override is remembered, so the user is warned about a
  // given machine once rather than on every launch.
  const overridden = localStorage.getItem(RAM_OVERRIDE_STORAGE_KEY) === 'true';

  // Accept without prompting when the model comfortably fits, when the user has
  // already opted in to running it anyway, or when the id no longer matches any
  // known model (in which case the backend, not this function, decides how to
  // handle it).
  if (!selected || overridden || hw.ram_gb >= selected.min_ram_gb) {
    localStorage.setItem(MODEL_STORAGE_KEY, selectedId);
    return selectedId;
  }

  // Underpowered machine: state the shortfall in concrete numbers and let the
  // user choose between degraded performance and a smaller model.
  const accepted = window.confirm(
    `Warning: Your system has ${hw.ram_gb.toFixed(1)}GB of RAM, but ${selected.name} requires at least ${selected.min_ram_gb}GB for optimal performance. You may experience slow downs or crashes.\n\nDo you want to continue anyway (allow override)?`
  );
  // Recording the override is what stops this prompt reappearing every launch.
  if (accepted) {
    localStorage.setItem(RAM_OVERRIDE_STORAGE_KEY, 'true');
    return selectedId;
  }

  // Declined: silently downgrading would be confusing, so the switch is both
  // persisted and announced, with a pointer to where it can be changed back.
  const fallback = lightestModel(models);
  localStorage.setItem(MODEL_STORAGE_KEY, fallback.id);
  alert(
    `Model automatically switched to a lighter version (${fallback.name}). You can change this in Settings.`
  );
  return fallback.id;
}

/**
 * Push the resolved model and concurrency settings into the backend at startup.
 *
 * Failures are logged and swallowed rather than propagated: the local LLM is an
 * optional enhancement, and an app that cannot configure it must still start.
 */
export async function syncLlmState(): Promise<void> {
  try {
    // Independent queries, so they run concurrently rather than in sequence.
    const [hw, models] = await Promise.all([
      API.llm.getHardwareInfo(),
      API.llm.getAvailableModels(),
    ]);

    // No models installed yet -- nothing to configure, and prompting about
    // hardware for a feature the user has not set up would be noise.
    if (models.length === 0) return;

    await API.llm.setActiveModel(resolveModelId(hw, models));

    // Parallel slots follow the same preference-then-recommendation pattern as
    // the model. clampSlots bounds the backend's suggestion to a range the UI
    // can represent, and the value is written back so the two stay in step.
    const initialSlots = storedSlots() ?? clampSlots(hw.recommended_slots);
    await API.llm.setParallelSlots(initialSlots);
    localStorage.setItem(PARALLEL_SLOTS_STORAGE_KEY, initialSlots.toString());
  } catch (e) {
    console.error('Failed to sync LLM state on startup', e);
  }
}
