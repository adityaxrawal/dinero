import { useState, useRef } from 'react';
import { useClickOutside } from '@/hooks/useClickOutside';

/** Calendar is ~320px tall; below this much room it flips above the trigger. */
const CALENDAR_HEIGHT = 330;

/** Open/closed state for the calendar, plus which way it should unfold. */
export function usePopover(disabled: boolean) {
  const [isOpen, setIsOpen] = useState(false);
  const [openUpward, setOpenUpward] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLDivElement>(null);

  const close = () => setIsOpen(false);
  useClickOutside(containerRef, close, isOpen);

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
