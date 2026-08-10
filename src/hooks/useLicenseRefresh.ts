import { useState } from 'react';
import { API } from '@/lib/ipc';

/**
 * Re-validate the license against the backend, with a pending flag for the UI.
 *
 * Errors are logged rather than surfaced: the result reaches the app through the
 * license store's event subscription, and a failed refresh simply leaves the
 * previous state in place.
 */
export function useLicenseRefresh() {
  const [isRetrying, setIsRetrying] = useState(false);

  /** Re-validates the licence, tracking the pending state for the UI. */
  const handleRetry = async () => {
    setIsRetrying(true);
    try {
      await API.licensing.refresh();
    } catch (err) {
      console.error('License refresh failed:', err);
    } finally {
      setIsRetrying(false);
    }
  };

  return { isRetrying, handleRetry };
}
