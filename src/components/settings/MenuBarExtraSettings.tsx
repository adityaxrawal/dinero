/**
 * Toggles the macOS menu-bar extra and its at-a-glance figures.
 */
import { useState, useEffect } from 'react';
import { AppWindow } from 'lucide-react';
import { API } from '@/lib/ipc';

/** Toggles the macOS menu-bar extra. */
export default function MenuBarExtraSettings() {
  const [enabled, setEnabled] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);

  useEffect(() => {
    API.menuBarExtra
      .getEnabled()
      .then(setEnabled)
      .catch((e) => console.error('Failed to load menu bar extra setting:', e))
      .finally(() => setIsLoading(false));
  }, []);

  /** Applies the preference immediately, without a save step. */
  const handleToggle = async (next: boolean) => {
    setIsSaving(true);
    setEnabled(next);
    try {
      await API.menuBarExtra.setEnabled(next);
    } catch (e) {
      console.error('Failed to update menu bar extra setting:', e);
      setEnabled(!next);
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <div className="space-y-5">
      <div className="flex items-center gap-2 mb-4">
        <AppWindow className="w-5 h-5 text-emerald-ink" />
        <h3 className="text-xl font-bold text-emerald-ink">Menu Bar Extra</h3>
      </div>
      <p className="text-sm text-emerald-ink/85 mb-5">
        Show a quick summary (month spend, pending review count, upcoming bills) in the macOS menu
        bar, with quick actions.
      </p>
      <label className="flex items-center gap-2 text-[13px] font-semibold text-emerald-ink cursor-pointer">
        <input
          type="checkbox"
          checked={enabled}
          disabled={isLoading || isSaving}
          onChange={(e) => handleToggle(e.target.checked)}
          aria-label="Show menu bar extra"
          className="w-4 h-4 border-emerald-ink/20 rounded bg-champagne text-emerald-ink focus:ring-emerald-ink"
        />
        Show menu bar extra
      </label>
    </div>
  );
}
