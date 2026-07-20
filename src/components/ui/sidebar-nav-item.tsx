import * as React from "react"
import { cn } from "@/lib/utils"

export interface SidebarNavItemProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  icon?: React.ReactNode
  label: React.ReactNode
  isSelected?: boolean
}

export const SidebarNavItem = React.forwardRef<HTMLButtonElement, SidebarNavItemProps>(
  ({ className, icon, label, isSelected, ...props }, ref) => {
    return (
      <button
        ref={ref}
        className={cn(
          "flex items-center gap-2 w-full text-left px-4 py-2.5 mx-2 rounded-md transition-colors max-w-[calc(100%-16px)] cursor-pointer select-none",
          isSelected
            ? "bg-[#064E3B] text-[#F8E7C9]"
            : "hover:bg-[#064E3B]/5 text-[#064E3B]",
          className
        )}
        {...props}
      >
        {icon && (
          <div className={cn("shrink-0 flex items-center justify-center", isSelected ? "text-[#F8E7C9]" : "text-[#064E3B]")}>
            {icon}
          </div>
        )}
        <span className={cn("text-[13px] font-semibold", isSelected ? "text-white" : "text-[#064E3B]")}>
          {label}
        </span>
      </button>
    )
  }
)
SidebarNavItem.displayName = "SidebarNavItem"
