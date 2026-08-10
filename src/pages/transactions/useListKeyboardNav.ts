/**
 * Keyboard navigation for the transaction list.
 *
 * Makes the ledger traversable without a mouse, which matters for a list users
 * scan in bulk.
 */
import { useState, useEffect } from 'react';

/** Keyboard navigation for the transaction list, so it is traversable without a mouse. */
export function useListKeyboardNav<T extends { id: string }>(items: T[]) {
  const [selectedId, setSelectedId] = useState<string | null>(null);

  useEffect(() => {
    /** Moves the selection with the arrow keys and opens on Enter. */
    const handleKeyDown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (
        target &&
        (target.tagName === 'INPUT' ||
          target.tagName === 'TEXTAREA' ||
          target.tagName === 'SELECT' ||
          target.isContentEditable)
      ) {
        return;
      }
      if (!items || items.length === 0) return;

      const currentIndex = selectedId ? items.findIndex((t) => t.id === selectedId) : -1;

      if (e.key === 'ArrowDown' || e.key === 'j') {
        e.preventDefault();
        const nextIndex = Math.min(items.length - 1, currentIndex + 1);
        if (items[nextIndex]) setSelectedId(items[nextIndex].id);
      } else if (e.key === 'ArrowUp' || e.key === 'k') {
        e.preventDefault();
        const prevIndex = Math.max(0, currentIndex - 1);
        if (items[prevIndex]) setSelectedId(items[prevIndex].id);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [items, selectedId]);

  return [selectedId, setSelectedId] as const;
}
