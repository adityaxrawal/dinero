import { useState, useEffect, useRef } from 'react';
import { useSearchParams } from 'react-router-dom';
import type { TransactionListFilters } from '@/lib/ipc';

export const ALL = '__all__';

export function useTransactionFilters() {
  const [searchParams] = useSearchParams();

  // Initialise filters from URL params (deep-link support from dashboard
  // categories / instrument detail)
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

  // Global shortcut to focus search input (Cmd+K / Ctrl+K)
  useEffect(() => {
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
