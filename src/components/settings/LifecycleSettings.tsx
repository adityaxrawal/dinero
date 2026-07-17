import { useState, useEffect } from 'react';
import { Power, RefreshCw } from 'lucide-react';
import { API } from '@/lib/ipc';

/**
 * TASK-DESK-010 (Doc 30 §12): "Launch at Login" (a real macOS Launch Agent,
 * via `tauri_plugin_autostart`) and "Continue syncing when app is closed"
 * (background-only mode, throttled polling on low battery). Native
 * checkboxes/inputs, matching MenuBarExtraSettings' established precedent
 * of not introducing a new Switch primitive for a handful of toggles.
 */
export default function LifecycleSettings() {
  const [launchAtLogin, setLaunchAtLogin] = useState(false);
  const [backgroundSync, setBackgroundSync] = useState(false);
  const [thresholdPercent, setThresholdPercent] = useState(20);
  const [isLoading, setIsLoading] = useState(true);
  const [isSavingLaunch, setIsSavingLaunch] = useState(false);
  const [isSavingSync, setIsSavingSync] = useState(false);

  useEffect(() => {
    Promise.all([
      API.lifecycle.getLaunchAtLogin(),
      API.lifecycle.getBackgroundSyncEnabled(),
      API.lifecycle.getLowBatteryPollThresholdPercent(),
    ])
      .then(([login, sync, threshold]) => {
        setLaunchAtLogin(login);
        setBackgroundSync(sync);
        setThresholdPercent(threshold);
      })
      .catch((e) => console.error('Failed to load lifecycle settings:', e))
      .finally(() => setIsLoading(false));
  }, []);

  const handleToggleLaunchAtLogin = async (next: boolean) => {
    setIsSavingLaunch(true);
    setLaunchAtLogin(next);
    try {
      await API.lifecycle.setLaunchAtLogin(next);
    } catch (e) {
      console.error('Failed to update launch-at-login setting:', e);
      setLaunchAtLogin(!next);
    } finally {
      setIsSavingLaunch(false);
    }
  };

  const handleToggleBackgroundSync = async (next: boolean) => {
    setIsSavingSync(true);
    setBackgroundSync(next);
    try {
      await API.lifecycle.setBackgroundSyncEnabled(next);
    } catch (e) {
      console.error('Failed to update background-sync setting:', e);
      setBackgroundSync(!next);
    } finally {
      setIsSavingSync(false);
    }
  };

  const handleThresholdChange = async (next: number) => {
    setThresholdPercent(next);
    try {
      await API.lifecycle.setLowBatteryPollThresholdPercent(next);
    } catch (e) {
      console.error('Failed to update low-battery threshold:', e);
    }
  };

  return (
    <div className="rounded-lg border border-border bg-card p-6 space-y-5">
      <div className="flex items-center gap-2">
        <Power className="w-5 h-5 text-muted-foreground" aria-hidden="true" />
        <h3 className="text-lg font-semibold">Startup & Background Sync</h3>
      </div>

      <label className="flex items-center gap-2 text-sm font-medium text-foreground">
        <input
          type="checkbox"
          checked={launchAtLogin}
          disabled={isLoading || isSavingLaunch}
          onChange={(e) => handleToggleLaunchAtLogin(e.target.checked)}
          aria-label="Launch Dinero at login"
          className="w-4 h-4 border border-border rounded"
          style={{ accentColor: 'var(--accent-primary)' }}
        />
        Launch Dinero at login
      </label>

      <div className="space-y-2">
        <label className="flex items-center gap-2 text-sm font-medium text-foreground">
          <input
            type="checkbox"
            checked={backgroundSync}
            disabled={isLoading || isSavingSync}
            onChange={(e) => handleToggleBackgroundSync(e.target.checked)}
            aria-label="Continue syncing when app is closed"
            className="w-4 h-4 border border-border rounded"
            style={{ accentColor: 'var(--accent-primary)' }}
          />
          Continue syncing when app is closed
        </label>
        <p className="text-sm text-muted-foreground pl-6">
          When enabled, closing the window keeps Dinero running in the background (no Dock icon)
          so Gmail syncing continues. When disabled, closing the window quits Dinero.
        </p>
      </div>

      {backgroundSync && (
        <div className="flex items-center gap-2 pl-6">
          <RefreshCw className="w-4 h-4 text-muted-foreground" aria-hidden="true" />
          <label htmlFor="low-battery-threshold" className="text-sm text-foreground">
            Slow down background syncing below
          </label>
          <input
            id="low-battery-threshold"
            type="number"
            min={0}
            max={100}
            step={5}
            value={thresholdPercent}
            disabled={isLoading}
            onChange={(e) => handleThresholdChange(Number(e.target.value))}
            className="w-16 rounded border border-border bg-background px-2 py-1 text-sm"
          />
          <span className="text-sm text-muted-foreground">% battery on AC-disconnected</span>
        </div>
      )}
    </div>
  );
}
