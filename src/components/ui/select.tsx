/**
 * Select control built on Radix Select.
 *
 * A native <select> cannot be styled consistently across platforms, so Radix
 * supplies an accessible listbox with full keyboard support and this layer
 * styles it. The scroll buttons exist for option lists taller than the viewport.
 */
import * as React from 'react';
import * as SelectPrimitive from '@radix-ui/react-select';
import { Check, ChevronDown, ChevronUp } from 'lucide-react';

import { cn } from '@/lib/utils';

/** Root select, owning open state and the selected value. */
const Select = SelectPrimitive.Root;

/** Displays the current selection, or the placeholder when empty. */
const SelectValue = SelectPrimitive.Value;

/** The button that opens the listbox and shows the current value. */
const SelectTrigger = React.forwardRef<
  React.ElementRef<typeof SelectPrimitive.Trigger>,
  React.ComponentPropsWithoutRef<typeof SelectPrimitive.Trigger> & { hideChevron?: boolean }
>(({ className, children, hideChevron = false, ...props }, ref) => (
  <SelectPrimitive.Trigger
    ref={ref}
    className={cn(
      'flex h-10 w-full items-center justify-between whitespace-nowrap rounded-md px-3 py-2 text-sm shadow-sm',
      'border border-border bg-background',
      'data-[placeholder]:text-muted-foreground/60',
      'transition-all duration-150 ease-out',
      'hover:border-[#064E3B]/30 hover:bg-accent',
      'data-[state=open]:border-[#064E3B]/60 data-[state=open]:bg-[#064E3B]/[0.04]',
      'data-[state=open]:ring-2 data-[state=open]:ring-[#064E3B]/30',
      'focus:outline-none focus:ring-2 focus:ring-[#064E3B]/60 focus:ring-offset-0 focus:border-[#064E3B]/60',
      'disabled:cursor-not-allowed disabled:opacity-40',
      '[&>span]:line-clamp-1',
      className
    )}
    {...props}
  >
    {children}
    {!hideChevron && (
      <SelectPrimitive.Icon asChild>
        <ChevronDown className="h-4 w-4 opacity-50 transition-transform duration-200 data-[state=open]:rotate-180" />
      </SelectPrimitive.Icon>
    )}
  </SelectPrimitive.Trigger>
));
SelectTrigger.displayName = SelectPrimitive.Trigger.displayName;

/** Scroll affordance shown when options extend above the viewport. */
const SelectScrollUpButton = React.forwardRef<
  React.ElementRef<typeof SelectPrimitive.ScrollUpButton>,
  React.ComponentPropsWithoutRef<typeof SelectPrimitive.ScrollUpButton>
>(({ className, ...props }, ref) => (
  <SelectPrimitive.ScrollUpButton
    ref={ref}
    className={cn('flex cursor-default items-center justify-center py-1', className)}
    {...props}
  >
    <ChevronUp className="h-4 w-4" />
  </SelectPrimitive.ScrollUpButton>
));
SelectScrollUpButton.displayName = SelectPrimitive.ScrollUpButton.displayName;

/** Scroll affordance shown when options extend below the viewport. */
const SelectScrollDownButton = React.forwardRef<
  React.ElementRef<typeof SelectPrimitive.ScrollDownButton>,
  React.ComponentPropsWithoutRef<typeof SelectPrimitive.ScrollDownButton>
>(({ className, ...props }, ref) => (
  <SelectPrimitive.ScrollDownButton
    ref={ref}
    className={cn('flex cursor-default items-center justify-center py-1', className)}
    {...props}
  >
    <ChevronDown className="h-4 w-4" />
  </SelectPrimitive.ScrollDownButton>
));
SelectScrollDownButton.displayName = SelectPrimitive.ScrollDownButton.displayName;

/**
 * The popover listbox.
 *
 * Portalled so it is not clipped by an ancestor's overflow, and positioned by
 * Radix so it flips rather than overflowing the window near a screen edge.
 */
const SelectContent = React.forwardRef<
  React.ElementRef<typeof SelectPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof SelectPrimitive.Content> & { hideScrollButtons?: boolean }
>(({ className, children, position = 'popper', hideScrollButtons = true, ...props }, ref) => (
  <SelectPrimitive.Portal>
    <SelectPrimitive.Content
      ref={ref}
      className={cn(
        'relative z-50 max-h-[--radix-select-content-available-height] min-w-[8rem] overflow-y-auto overflow-x-hidden',
        'rounded-xl border border-border bg-popover text-popover-foreground shadow-lg',
        'data-[state=open]:animate-in data-[state=closed]:animate-out',
        'data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0',
        'data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95',
        'data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2',
        'data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2',
        'origin-[--radix-select-content-transform-origin]',
        position === 'popper' &&
          'data-[side=bottom]:translate-y-1 data-[side=left]:-translate-x-1 data-[side=right]:translate-x-1 data-[side=top]:-translate-y-1',
        className
      )}
      position={position}
      {...props}
    >
      {!hideScrollButtons && <SelectScrollUpButton />}
      <SelectPrimitive.Viewport
        className={cn(
          'p-1.5',
          position === 'popper' &&
            'h-[var(--radix-select-trigger-height)] w-full min-w-[var(--radix-select-trigger-width)]'
        )}
      >
        {children}
      </SelectPrimitive.Viewport>
      {!hideScrollButtons && <SelectScrollDownButton />}
    </SelectPrimitive.Content>
  </SelectPrimitive.Portal>
));
SelectContent.displayName = SelectPrimitive.Content.displayName;

/** Heading for a group of options. */
const SelectLabel = React.forwardRef<
  React.ElementRef<typeof SelectPrimitive.Label>,
  React.ComponentPropsWithoutRef<typeof SelectPrimitive.Label>
>(({ className, ...props }, ref) => (
  <SelectPrimitive.Label
    ref={ref}
    className={cn(
      'px-2 py-1.5 text-xs font-semibold text-muted-foreground uppercase tracking-wider',
      className
    )}
    {...props}
  />
));
SelectLabel.displayName = SelectPrimitive.Label.displayName;

/** One selectable option, with its selected-state indicator. */
const SelectItem = React.forwardRef<
  React.ElementRef<typeof SelectPrimitive.Item>,
  React.ComponentPropsWithoutRef<typeof SelectPrimitive.Item> & { hideCheckmark?: boolean }
>(({ className, children, hideCheckmark = false, ...props }, ref) => (
  <SelectPrimitive.Item
    ref={ref}
    className={cn(
      'relative flex w-full cursor-pointer select-none items-center rounded-lg py-2 pl-3 pr-9 text-sm outline-none',
      'text-muted-foreground',
      'transition-all duration-100 ease-out',
      'focus:bg-[#064E3B]/10 focus:text-foreground',
      'hover:bg-accent hover:text-foreground',
      'data-[state=checked]:bg-[#064E3B]/10 data-[state=checked]:text-[#053d2f] data-[state=checked]:font-medium',
      'data-[state=checked]:border-l-2 data-[state=checked]:border-l-[#064E3B] data-[state=checked]:pl-[10px]',
      'data-[disabled]:pointer-events-none data-[disabled]:opacity-30',
      className
    )}
    {...props}
  >
    {!hideCheckmark && (
      <span className="absolute right-2.5 flex h-4 w-4 items-center justify-center">
        <SelectPrimitive.ItemIndicator>
          <Check className="h-3.5 w-3.5 text-[#064E3B]" strokeWidth={3} />
        </SelectPrimitive.ItemIndicator>
      </span>
    )}
    <SelectPrimitive.ItemText>{children}</SelectPrimitive.ItemText>
  </SelectPrimitive.Item>
));
SelectItem.displayName = SelectPrimitive.Item.displayName;

/** Groups related options under a shared label. */
const SelectGroup = SelectPrimitive.Group;

export { Select, SelectValue, SelectTrigger, SelectContent, SelectItem, SelectGroup, SelectLabel };
