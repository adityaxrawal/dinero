import { useState } from 'react';
import { API } from '@/lib/ipc';

export function useLicenseRefresh() {
  const [isRetrying, setIsRetrying] = useState(false);

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
