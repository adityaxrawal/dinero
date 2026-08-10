/**
 * Loads the raw source payload for a transaction on demand.
 */
import { useState } from 'react';
import { API } from '@/lib/ipc';

/** Loads the raw source payload on demand. */
export function useRawSource(id: string | undefined) {
  const [isOpen, setIsOpen] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [data, setData] = useState<unknown>(null);

  /** Opens the dialog, fetching the payload if not already loaded. */
  const open = async () => {
    if (!id) return;
    setIsOpen(true);
    setIsLoading(true);
    try {
      const sourceLog = await API.transactions.getSourceLog(id);
      setData(sourceLog || { error: 'No source data found for this transaction.' });
    } catch {
      setData({ error: 'Failed to load source data.' });
    } finally {
      setIsLoading(false);
    }
  };

  return { isOpen, setIsOpen, isLoading, data, open };
}
