/**
 * Shared sizing and styling constants for the inspector side panels.
 */
import { cn } from '@/lib/utils';

/** Shared classes for inspector side panels. */
export function inspectorPanelClasses(inline: boolean, isOpen: boolean): string {
  return cn(
    !inline && 'inspector-panel',
    !inline && !isOpen && 'closed',
    inline && 'w-full h-full flex flex-col',
    !inline && 'flex-shrink-0'
  );
}

/** Shared inline sizing for inspector side panels. */
export function inspectorPanelStyle(inline: boolean, isOpen: boolean): React.CSSProperties {
  return inline
    ? { backgroundColor: '#F8E7C9' }
    : { width: isOpen ? 'var(--inspector-width)' : 0 };
}
