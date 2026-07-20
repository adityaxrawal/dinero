import { useState } from 'react';
import { useIpcListen } from '@/hooks/useIpcListen';
import { AlertTriangle, BellOff, X } from 'lucide-react';
import { Button } from '@/components/ui/button';

interface SystemWarningPayload {
  warning_type: string;
  message: string;
  severity: 'hard_fail' | 'soft_fail';
}

const KEYCHAIN_DENIED = 'keychain_denied';
const NOTIFICATION_DENIED = 'notification_denied';

/**
 * TASK-DESK-004 (Doc 30 §12): OS-level permission denial UI states, driven
 * by the backend's `system_warning` event (Document 19 §15) carrying this
 * task's `warning_type`/`severity` values.
 *
 * - **Keychain denied** (`hard_fail`): a persistent, full-screen,
 *   non-dismissable overlay -- Gmail tokens and the SQLite encryption key
 *   both live in Keychain, so there is no safe degraded mode to fall back
 *   to. A direct System Settings deep link, not just instructions.
 * - **Notification permission denied** (`soft_fail`): a small, dismissable,
 *   non-blocking note -- core functionality is unaffected.
 *
 * File/folder access denial is deliberately not handled here: it's
 * necessarily reactive (macOS TCC grants per-path at the moment of a real
 * read), so it's a dismissable toast at the specific failed upload attempt
 * instead (`StatementUploadDropzone.tsx`), not a global state this
 * component would need to track.
 */
export default function PermissionDeniedOverlay() {
  const [keychainDenied, setKeychainDenied] = useState<string | null>(null);
  const [notificationDenied, setNotificationDenied] = useState<string | null>(null);
  const [notificationDismissed, setNotificationDismissed] = useState(false);

  useIpcListen<SystemWarningPayload>('system_warning', (payload) => {
    const { warning_type, message } = payload;
    if (warning_type === KEYCHAIN_DENIED) {
      setKeychainDenied(message);
    } else if (warning_type === NOTIFICATION_DENIED) {
      setNotificationDenied(message);
      setNotificationDismissed(false);
    }
  });

  const openSystemSettings = async (pane: string) => {
    try {
      const { openUrl } = await import('@tauri-apps/plugin-opener');
      await openUrl(`x-apple.systempreferences:${pane}`);
    } catch (e) {
      console.error('Failed to open System Settings', e);
    }
  };

  return (
    <>
      {keychainDenied && (
        <div
          role="alertdialog"
          aria-modal="true"
          aria-labelledby="keychain-denied-title"
          aria-describedby="keychain-denied-desc"
          data-testid="permission-denied-overlay"
          className="fixed inset-0 z-[100] flex items-center justify-center bg-background/95 backdrop-blur-sm"
        >
          <div className="max-w-md w-full mx-4 bg-card border border-border rounded-lg p-6 flex flex-col items-center text-center space-y-4 shadow-2xl">
            <div className="h-12 w-12 rounded-full bg-destructive/20 flex items-center justify-center">
              <AlertTriangle className="text-red-700 w-6 h-6" aria-hidden="true" />
            </div>
            <h2 id="keychain-denied-title" className="text-xl font-semibold">
              Keychain Access Required
            </h2>
            <p id="keychain-denied-desc" className="text-sm text-muted-foreground">
              {keychainDenied}
            </p>
            <Button
              variant="default"
              onClick={() => openSystemSettings('com.apple.preference.security?Privacy')}
              aria-label="Open System Settings to grant Keychain access"
            >
              Open System Settings
            </Button>
          </div>
        </div>
      )}

      {notificationDenied && !notificationDismissed && (
        <div
          role="status"
          aria-live="polite"
          data-testid="notification-permission-note"
          className="mb-4 p-3 rounded-md bg-secondary border border-border flex items-start gap-3"
        >
          <BellOff className="w-4 h-4 text-muted-foreground shrink-0 mt-0.5" aria-hidden="true" />
          <div className="flex-1 min-w-0 text-xs text-muted-foreground">{notificationDenied}</div>
          <button
            type="button"
            onClick={() => openSystemSettings('com.apple.preference.notifications')}
            className="text-xs font-medium text-foreground underline shrink-0"
          >
            Open Settings
          </button>
          <button
            type="button"
            onClick={() => setNotificationDismissed(true)}
            aria-label="Dismiss notification permission note"
            className="shrink-0"
          >
            <X className="w-3.5 h-3.5 text-muted-foreground" aria-hidden="true" />
          </button>
        </div>
      )}
    </>
  );
}
