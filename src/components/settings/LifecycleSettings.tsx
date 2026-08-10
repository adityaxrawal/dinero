/**
 * Controls launch-at-login, background sync, and close-versus-quit behaviour.
 *
 * These decide whether ingestion continues while the window is closed, which is
 * what makes transactions appear without the app being open.
 */
import { useState, useEffect } from 'react';
import { Power, RefreshCw } from 'lucide-react';
import { API } from '@/lib/ipc';

/** Launch-at-login, background sync, and battery thresholds. */
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

  /** Registers or removes the login item. */
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

  /** Enables background sync, which keeps ingestion running when the window closes. */
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

  /** Sets the battery level below which polling slows. */
  const handleThresholdChange = async (next: number) => {
    setThresholdPercent(next);
    try {
      await API.lifecycle.setLowBatteryPollThresholdPercent(next);
    } catch (e) {
      console.error('Failed to update low-battery threshold:', e);
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-2 mb-4">
        <Power className="w-5 h-5 text-[#064E3B]" aria-hidden="true" />
        <h3 className="text-xl font-bold text-[#064E3B]">Startup & Background Sync</h3>
      </div>

      <label className="flex items-center gap-2 text-[13px] font-semibold text-[#064E3B] cursor-pointer">
        <input
          type="checkbox"
          checked={launchAtLogin}
          disabled={isLoading || isSavingLaunch}
          onChange={(e) => handleToggleLaunchAtLogin(e.target.checked)}
          aria-label="Launch Dinero at login"
          className="w-4 h-4 border-[#064E3B]/20 rounded bg-[#F8E7C9] text-[#064E3B] focus:ring-[#064E3B]"
        />
        Launch Dinero at login
      </label>

      <div className="space-y-2">
        <label className="flex items-center gap-2 text-[13px] font-semibold text-[#064E3B] cursor-pointer">
          <input
            type="checkbox"
            checked={backgroundSync}
            disabled={isLoading || isSavingSync}
            onChange={(e) => handleToggleBackgroundSync(e.target.checked)}
            aria-label="Continue syncing when app is closed"
            className="w-4 h-4 border-[#064E3B]/20 rounded bg-[#F8E7C9] text-[#064E3B] focus:ring-[#064E3B]"
          />
          Continue syncing when app is closed
        </label>
        <p className="text-[13px] font-medium text-[#064E3B]/70 pl-6">
          When enabled, closing the window keeps Dinero running in the background (no Dock icon) so
          Gmail syncing continues. When disabled, closing the window quits Dinero.
        </p>
      </div>

      {backgroundSync && (
        <div className="flex items-center gap-2 pl-6 mt-4">
          <RefreshCw className="w-4 h-4 text-[#064E3B]/50" aria-hidden="true" />
          <label
            htmlFor="low-battery-threshold"
            className="text-[13px] font-semibold text-[#064E3B]"
          >
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
            className="w-16 px-2 py-1.5 rounded-lg border text-[13px] font-medium bg-[#F8E7C9]/50 border-[#064E3B]/20 text-[#064E3B] focus:border-[#064E3B] focus:ring-1 focus:ring-[#064E3B]"
          />
          <span className="text-[13px] font-medium text-[#064E3B]/70">
            % battery on AC-disconnected
          </span>
        </div>
      )}
    </div>
  );
}
