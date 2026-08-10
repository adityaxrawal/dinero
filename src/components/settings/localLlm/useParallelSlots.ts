/**
 * Reads and persists the parallel-slot setting, clamped to the safe ceiling.
 */
import { useState } from 'react';
import { API } from '@/lib/ipc';
import { clampSlots, storedSlots, PARALLEL_SLOTS_STORAGE_KEY } from './format';

/** Reads and persists the parallel-slot setting. */
export function useParallelSlots() {
  const [savedSlots, setSavedSlots] = useState<number>(() => storedSlots() ?? 1);
  const [draftSlots, setDraftSlots] = useState<number>(() => storedSlots() ?? 1);
  const [isSaving, setIsSaving] = useState(false);
  const [justSaved, setJustSaved] = useState(false);

  /** Adopts the backend's hardware-derived recommendation. */
  const adoptDefault = (recommended: number) => {
    const initial = storedSlots() ?? recommended;
    setSavedSlots(initial);
    setDraftSlots(initial);
    return initial;
  };

  /** Persists the chosen slot count, clamped to the safe ceiling. */
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
