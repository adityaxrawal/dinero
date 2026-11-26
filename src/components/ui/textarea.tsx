/**
 * Multi-line text input, styled to match the single-line Input.
 */
import * as React from 'react';

import { cn } from '@/lib/utils';

/** Multi-line input matching the single-line Input's styling. */
const Textarea = React.forwardRef<HTMLTextAreaElement, React.ComponentProps<'textarea'>>(
  ({ className, ...props }, ref) => {
    return (
      <textarea
        className={cn(
          'flex min-h-[60px] w-full rounded-md border bg-transparent px-3 py-2 text-sm shadow-sm',
          'border-border bg-background',
          'placeholder:text-muted-foreground/50',
          'transition-all duration-150 ease-out',
          'hover:border-emerald-ink/30 hover:bg-accent',
          'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-ink/60 focus-visible:ring-offset-0',
          'focus-visible:border-emerald-ink/60 focus-visible:bg-emerald-ink/[0.04]',
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
Textarea.displayName = 'Textarea';

export { Textarea };
