import { useEffect, useState } from 'react';

/**
 * A once-per-second clock, running only while `active`.
 *
 * Powers live elapsed-time displays. Gated deliberately: an unconditional
 * interval would re-render its consumers every second forever, including on
 * screens where nothing is timing. The immediate `setNow` on activation avoids
 * showing a stale timestamp for the first second after it switches on.
 */
export function useNowTicker(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const interval = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(interval);
  }, [active]);

  return now;
}
