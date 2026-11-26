/**
 * Reconciles the user's saved local-LLM preferences with the machine's actual
 * hardware at application startup.
 *
 * The stored model choice is a preference, not a guarantee: the same profile can
 * follow a user onto a machine with less RAM, and a model that ran acceptably
 * before may now thrash or crash the process. Rather than failing at inference
 * time, this module validates the choice up front and, when the machine cannot
 * support it, switches to the lightest model and offers "Use anyway".
 *
 * The model preference and the RAM override live in the database
 * (`local_profile.llm_model`, `local_profile.llm_ram_override`), read through
 * the backend, so the Rust readiness check and this UI share one owner
 * (audits B-3, F7-5). localStorage holds only the slot count.
 */
import { API, type LlmModelInfo } from '@/lib/ipc';
import { toast } from '@/hooks/use-toast';
import { ToastAction } from '@/components/ui/toast';
import {
  LEGACY_RAM_OVERRIDE_STORAGE_KEY,
  PARALLEL_SLOTS_STORAGE_KEY,
  clampSlots,
  storedSlots,
} from '@/components/settings/localLlm/format';
import { getErrorToast } from '@/lib/errorMapping';

/** The model with the smallest RAM requirement -- the safe fallback for any machine. */
function lightestModel(models: LlmModelInfo[]): LlmModelInfo {
  return models.reduce((a, b) => (a.min_ram_gb <= b.min_ram_gb ? a : b));
}

/**
 * One-time move of a RAM override saved by older builds in localStorage into
 * the backend setting (audit F7-5). The key is removed either way.
 */
async function migrateLegacyRamOverride(): Promise<void> {
  if (localStorage.getItem(LEGACY_RAM_OVERRIDE_STORAGE_KEY) === 'true') {
    await API.llm.setRamOverride(true);
  }
  localStorage.removeItem(LEGACY_RAM_OVERRIDE_STORAGE_KEY);
}

/**
 * Push the resolved model and concurrency settings into the backend at startup.
 *
 * Failures are logged and swallowed rather than propagated: the local LLM is an
 * optional enhancement, and an app that cannot configure it must still start.
 */
export async function syncLlmState(): Promise<void> {
  try {
    await migrateLegacyRamOverride();

    // Independent queries, so they run concurrently rather than in sequence.
    const [hw, models, preferred] = await Promise.all([
      API.llm.getHardwareInfo(),
      API.llm.getAvailableModels(),
      API.llm.getActiveModel().catch(() => ''),
    ]);

    // No models installed yet -- nothing to configure, and prompting about
    // hardware for a feature the user has not set up would be noise.
    if (models.length === 0) return;

    // Preference order: what the user last chose (stored in the database), else
    // the backend's hardware-based recommendation, else the first model.
    const selectedId = preferred || hw.recommended_model_id || models[0].id;
    const selected = models.find((m) => m.id === selectedId);

    // Accept when the model fits, when the user already opted in to running it
    // anyway, or when the id matches no known model (the backend decides then).
    if (!selected || hw.ram_override || hw.ram_gb >= selected.min_ram_gb) {
      await API.llm.setActiveModel(selectedId);
    } else {
      // Underpowered machine: switch to the lightest model without blocking
      // startup, and say so with a way back (audit F7-5).
      const fallback = lightestModel(models);
      await API.llm.setActiveModel(fallback.id);
      const outcome =
        fallback.id === selected.id
          ? 'Local AI stays off on this Mac.'
          : `Dinero switched to ${fallback.name}.`;
      toast({
        title: 'Not enough memory for this AI model',
        description: `This Mac has ${hw.ram_gb.toFixed(1)} GB of RAM; ${selected.name} needs at least ${selected.min_ram_gb} GB. ${outcome} You can change this in Settings.`,
        action: (
          <ToastAction
            altText="Use the model anyway"
            onClick={() => {
              API.llm
                .setRamOverride(true)
                .then(() => API.llm.setActiveModel(selected.id))
                .catch((e) => {
        console.error('Failed to apply the RAM override', e);
        toast({ variant: 'destructive', ...getErrorToast(e, 'Could not apply the memory override.') });
      });
            }}
          >
            Use anyway
          </ToastAction>
        ),
      });
    }

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
