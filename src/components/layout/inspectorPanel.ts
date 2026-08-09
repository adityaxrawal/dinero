import { cn } from '@/lib/utils';

/** The slide-out inspector shell, shared by the instrument and unassigned
 *  panels. `inline` renders the same content as a full-width page instead. */
export function inspectorPanelClasses(inline: boolean, isOpen: boolean): string {
  return cn(
    !inline && 'inspector-panel',
    !inline && !isOpen && 'closed',
    inline && 'w-full h-full flex flex-col',
    !inline && 'flex-shrink-0'
  );
}

export function inspectorPanelStyle(inline: boolean, isOpen: boolean): React.CSSProperties {
  return inline
    ? { backgroundColor: '#F8E7C9' }
    : { width: isOpen ? 'var(--inspector-width)' : 0 };
}
