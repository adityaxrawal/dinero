import * as React from "react"
import { Slot } from "@radix-ui/react-slot"
import { cva, type VariantProps } from "class-variance-authority"

import { cn } from "@/lib/utils"

const buttonVariants = cva(
  // Base — smooth transitions, stronger focus ring, accessible active state
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium " +
  "transition-all duration-200 ease-out " +
  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#2563eb] focus-visible:ring-offset-2 focus-visible:ring-offset-background " +
  "disabled:pointer-events-none disabled:opacity-40 " +
  "active:scale-[0.98] " +
  "[&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        default:
          "bg-primary text-primary-foreground shadow-sm " +
          "hover:bg-primary/85 hover:shadow-md " +
          "active:bg-primary/95",
        destructive:
          "bg-destructive/10 text-red-700 border border-destructive/30 shadow-sm " +
          "hover:bg-destructive/20 hover:text-red-800 hover:border-destructive/50 " +
          "active:bg-destructive/25",
        outline:
          "border border-border bg-transparent text-foreground shadow-sm " +
          "hover:bg-accent hover:text-foreground " +
          "active:bg-accent/70",
        secondary:
          "bg-secondary text-secondary-foreground shadow-sm " +
          "hover:bg-secondary/70 hover:shadow-md " +
          "active:bg-secondary/85",
        ghost:
          "text-muted-foreground " +
          "hover:bg-accent hover:text-foreground " +
          "active:bg-accent/70",
        link:
          "text-[#2563eb] underline-offset-4 hover:underline hover:text-[#1d4ed8]",
        // Accent/brand primary — Doc 14 §4.1's solid --primary, no gradient/glow
        accent:
          "bg-[#2563eb] text-white font-semibold border border-transparent " +
          "shadow-sm hover:bg-[#1d4ed8] hover:shadow-md " +
          "active:bg-[#1d4ed8]",
      },
      size: {
        default: "h-9 px-4 py-2",
        sm: "h-8 rounded-md px-3 text-xs",
        lg: "h-11 rounded-lg px-8 text-base",
        icon: "h-9 w-9",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  }
)

interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, ...props }, ref) => {
    const Comp = asChild ? Slot : "button"
    return (
      <Comp
        className={cn(buttonVariants({ variant, size, className }))}
        ref={ref}
        {...props}
      />
    )
  }
)
Button.displayName = "Button"

export { Button }
