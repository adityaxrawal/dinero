import { useState, useEffect } from 'react';
import { AppWindow } from 'lucide-react';
import { API } from '@/lib/ipc';

/**
 * TASK-DESK-008 (Doc 30 §12): "toggleable in Settings." A native checkbox
 * rather than a new shadcn Switch primitive -- this codebase has no
 * existing Switch component, and introducing one for a single toggle
 * would be disproportionate. Toggling calls
 * `settings_set_menu_bar_extra_enabled`, which immediately builds or
 * removes the real tray icon (`menu::status_item`) -- no restart needed.
 */
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
    <div className="rounded-lg border border-border bg-card p-6 space-y-3">
      <div className="flex items-center gap-2">
        <AppWindow className="w-5 h-5 text-muted-foreground" aria-hidden="true" />
        <h3 className="text-lg font-semibold">Menu Bar Extra</h3>
      </div>
      <p className="text-sm text-muted-foreground">
        Show a quick summary (month spend, pending review count, upcoming bills) in the macOS menu
        bar, with quick actions.
      </p>
      <label className="flex items-center gap-2 text-sm font-medium text-foreground">
        <input
          type="checkbox"
          checked={enabled}
          disabled={isLoading || isSaving}
          onChange={(e) => handleToggle(e.target.checked)}
          aria-label="Show menu bar extra"
          className="w-4 h-4 border border-border rounded"
          style={{ accentColor: 'var(--accent-primary)' }}
        />
        Show menu bar extra
      </label>
    </div>
  );
}
