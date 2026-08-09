import { useState } from 'react';
import { API } from '@/lib/ipc';
import { clampSlots, storedSlots, PARALLEL_SLOTS_STORAGE_KEY } from './format';

/**
 * `savedSlots` is the last value actually pushed to the backend/localStorage;
 * `draftSlots` is what's in the input right now. They diverge the moment the
 * user edits the input and stay diverged until Save is clicked — typing alone
 * never reaches the backend, avoiding a server restart on every keystroke
 * (each change forces `llama-server` to respawn, see `llama_sidecar.rs`'s
 * `ensure_server_ready`).
 */
export function useParallelSlots() {
  const [savedSlots, setSavedSlots] = useState<number>(() => storedSlots() ?? 1);
  const [draftSlots, setDraftSlots] = useState<number>(() => storedSlots() ?? 1);
  const [isSaving, setIsSaving] = useState(false);
  const [justSaved, setJustSaved] = useState(false);

  /** Called once hardware info lands, so the default is the recommendation. */
  const adoptDefault = (recommended: number) => {
    const initial = storedSlots() ?? recommended;
    setSavedSlots(initial);
    setDraftSlots(initial);
    return initial;
  };

  const save = async () => {
    const clamped = clampSlots(draftSlots);
    setIsSaving(true);
    try {
      await API.llm.setParallelSlots(clamped);
      localStorage.setItem(PARALLEL_SLOTS_STORAGE_KEY, String(clamped));
      setSavedSlots(clamped);
      setDraftSlots(clamped);
      setJustSaved(true);
      setTimeout(() => setJustSaved(false), 2000);
    } catch (err) {
      alert(`Failed to update parallel instances: ${err}`);
    } finally {
      setIsSaving(false);
    }
  };

  return {
    draftSlots,
    setDraftSlots,
    isDirty: draftSlots !== savedSlots,
    isSaving,
    justSaved,
    adoptDefault,
    save,
  };
}
