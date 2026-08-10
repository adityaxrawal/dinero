/**
 * Filter state for the transaction feed, including URL synchronisation.
 */
import { useState, useEffect, useRef } from 'react';
import { useSearchParams } from 'react-router-dom';
import type { TransactionListFilters } from '@/lib/ipc';

export const ALL = '__all__';

/** Filter state for the feed, synchronised with the URL. */
export function useTransactionFilters() {
  const [searchParams] = useSearchParams();

  const [filters, setFilters] = useState<TransactionListFilters>(() => {
    const category = searchParams.get('category');
    const instrument = searchParams.get('instrument');
    return {
      ...(category ? { category_id: category } : {}),
      ...(instrument ? { instrument_id: instrument } : {}),
    };
  });

  const [searchQuery, setSearchQuery] = useState('');
  const searchInputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    /** Focuses the search box on Cmd/Ctrl+K. */
    const handleCmdK = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        searchInputRef.current?.focus();
      }
    };
    window.addEventListener('keydown', handleCmdK);
    return () => window.removeEventListener('keydown', handleCmdK);
  }, []);

  const setFilter = <K extends keyof TransactionListFilters>(
    key: K,
    value: TransactionListFilters[K] | undefined
  ) => {
    setFilters((prev) => {
      const next = { ...prev };
      if (value === undefined || value === ALL) {
        delete next[key];
      } else {
        next[key] = value;
      }
      return next;
    });
  };

  return {
    filters,
    setFilters,
    setFilter,
    activeFilterCount: Object.values(filters).filter(Boolean).length,
    searchQuery,
    setSearchQuery,
    isSearching: searchQuery.trim().length > 0,
    searchInputRef,
  };
}
