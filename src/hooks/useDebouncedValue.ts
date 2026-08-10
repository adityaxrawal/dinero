import { useEffect, useState } from 'react';

/**
 * Delay propagating a rapidly changing value until it settles.
 *
 * Each new value restarts the timer, so the returned value only updates once
 * the input has been still for the full delay. Used for search input, where
 * querying on every keystroke would fire a request per character.
 */
export function useDebouncedValue<T>(value: T, delayMs: number): T {
  const [debounced, setDebounced] = useState(value);

  useEffect(() => {
    const timer = setTimeout(() => setDebounced(value), delayMs);
    return () => clearTimeout(timer);
  }, [value, delayMs]);

  return debounced;
}
