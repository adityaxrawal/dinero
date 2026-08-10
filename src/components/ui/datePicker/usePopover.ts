/**
 * Open/close state and outside-click dismissal for a popover.
 */
import { useState, useRef } from 'react';
import { useClickOutside } from '@/hooks/useClickOutside';

const CALENDAR_HEIGHT = 330;

/** Open state and outside-click dismissal for a popover. */
export function usePopover(disabled: boolean) {
  const [isOpen, setIsOpen] = useState(false);
  const [openUpward, setOpenUpward] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLDivElement>(null);

  /** Closes the popover. */
  const close = () => setIsOpen(false);
  useClickOutside(containerRef, close, isOpen);

  /** Toggles the popover open or closed. */
  const toggle = () => {
    if (disabled) return;
    if (!isOpen && triggerRef.current) {
      const rect = triggerRef.current.getBoundingClientRect();
      const spaceBelow = window.innerHeight - rect.bottom;
      setOpenUpward(spaceBelow < CALENDAR_HEIGHT && rect.top > spaceBelow);
    }
    setIsOpen(!isOpen);
  };

  return { isOpen, openUpward, containerRef, triggerRef, toggle, close };
}
