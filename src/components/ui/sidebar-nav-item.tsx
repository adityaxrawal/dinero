/**
 * One navigation entry in the sidebar: icon, label, active state and badge.
 *
 * Kept separate from the sidebar itself so the active and pulse styling live in
 * one place rather than being repeated per entry.
 */
import * as React from 'react';
import { cn } from '@/lib/utils';

interface SidebarNavItemProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  icon?: React.ReactNode;
  label: React.ReactNode;
  isSelected?: boolean;
}

/** One sidebar navigation entry: icon, label, active state, badge. */
export const SidebarNavItem = React.forwardRef<HTMLButtonElement, SidebarNavItemProps>(
  ({ className, icon, label, isSelected, ...props }, ref) => {
    return (
      <button
        ref={ref}
        className={cn(
          'flex items-center gap-2 w-full text-left px-4 py-2.5 mx-2 rounded-md transition-colors max-w-[calc(100%-16px)] cursor-pointer select-none',
          isSelected ? 'bg-emerald-ink text-champagne' : 'hover:bg-emerald-ink/5 text-emerald-ink',
          className
        )}
        {...props}
      >
        {icon && (
          <div
            className={cn(
              'shrink-0 flex items-center justify-center',
              isSelected ? 'text-champagne' : 'text-emerald-ink'
            )}
          >
            {icon}
          </div>
        )}
        <span
          className={cn('text-[13px] font-semibold', isSelected ? 'text-white' : 'text-emerald-ink')}
        >
          {label}
        </span>
      </button>
    );
  }
);
SidebarNavItem.displayName = 'SidebarNavItem';
