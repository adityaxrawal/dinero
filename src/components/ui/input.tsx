/**
 * Text input primitive with the app's shared field styling.
 *
 * Forwards its ref so callers can focus it imperatively and so form libraries
 * can register the underlying element.
 */
import * as React from 'react';

import { cn } from '@/lib/utils';

/** Text input with shared field styling. */
const Input = React.forwardRef<HTMLInputElement, React.ComponentProps<'input'>>(
  ({ className, type, ...props }, ref) => {
    return (
      <input
        type={type}
        className={cn(
          'flex h-10 w-full rounded-md border bg-transparent px-3 py-1 text-sm shadow-sm',
          'border-border bg-background',
          'placeholder:text-muted-foreground/50',
          'transition-all duration-150 ease-out',
          'hover:border-[#064E3B]/30 hover:bg-accent',
          'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#064E3B]/60 focus-visible:ring-offset-0',
          'focus-visible:border-[#064E3B]/60 focus-visible:bg-[#064E3B]/[0.04]',
          'file:border-0 file:bg-transparent file:text-sm file:font-medium file:text-foreground',
          'disabled:cursor-not-allowed disabled:opacity-40',
          'aria-invalid:border-red-500/60 aria-invalid:ring-red-500/30',
          className
        )}
        ref={ref}
        {...props}
      />
    );
  }
);
Input.displayName = 'Input';

export { Input };
