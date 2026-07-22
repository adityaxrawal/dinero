import * as React from "react"

import { cn } from "@/lib/utils"

const Textarea = React.forwardRef<HTMLTextAreaElement, React.ComponentProps<"textarea">>(
  ({ className, ...props }, ref) => {
    return (
      <textarea
        className={cn(
          // Base
          "flex min-h-[60px] w-full rounded-md border bg-transparent px-3 py-2 text-sm shadow-sm",
          // Border / background
          "border-border bg-background",
          // Placeholder
          "placeholder:text-muted-foreground/50",
          // Transitions
          "transition-all duration-150 ease-out",
          // Hover
          "hover:border-[#064E3B]/30 hover:bg-accent",
          // Focus — Emerald Ink ring + tinted background
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#064E3B]/60 focus-visible:ring-offset-0",
          "focus-visible:border-[#064E3B]/60 focus-visible:bg-[#064E3B]/[0.04]",
          // Disabled
          "disabled:cursor-not-allowed disabled:opacity-40",
          // Aria invalid
          "aria-invalid:border-red-500/60 aria-invalid:ring-red-500/30",
          className
        )}
        ref={ref}
        {...props}
      />
    )
  }
)
Textarea.displayName = "Textarea"

export { Textarea }
