import { useEffect } from 'react';

/**
 * Invoke a callback when a click lands outside the referenced element.
 *
 * Closes popovers and dropdowns. Listens on `mousedown` rather than `click`, so
 * the dismissal happens on press -- a `click` listener would fire after the
 * press had already moved focus, and would miss a drag that ends elsewhere.
 *
 * The dependency list intentionally tracks only `enabled`: re-subscribing every
 * time an inline callback identity changed would detach and reattach the
 * listener on each render for no benefit.
 */
export function useClickOutside(
  ref: React.RefObject<HTMLElement | null>,
  onOutside: () => void,
  enabled: boolean
) {
  useEffect(() => {
    if (!enabled) return;
    /** Fires the callback when a press lands outside the referenced element. */
    function handleClickOutside(event: MouseEvent) {
      if (ref.current && !ref.current.contains(event.target as Node)) onOutside();
    }
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled]);
}
