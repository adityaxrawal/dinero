import { useState } from 'react';
import { API } from '@/lib/ipc';

/** The "View Raw Source" dialog: fetched on demand, never with the page. */
export function useRawSource(id: string | undefined) {
  const [isOpen, setIsOpen] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [data, setData] = useState<unknown>(null);

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
