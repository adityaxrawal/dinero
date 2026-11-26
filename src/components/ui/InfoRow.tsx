/**
 * Label-and-value row used throughout the detail panels.
 *
 * Exists so that every inspector aligns its fields identically instead of each
 * panel inventing its own two-column layout.
 */
import React, { useState } from 'react';
import { Copy, Check } from 'lucide-react';
import { cn } from '@/lib/utils';

/** Label-and-value row with an optional copy affordance. */
export function InfoRow({
  icon,
  label,
  children,
  copyValue,
  className,
}: {
  icon?: React.ReactNode;
  label: string;
  children: React.ReactNode;
  copyValue?: string;
  className?: string;
}) {
  const [copied, setCopied] = useState(false);

  /** Copies the value, confirming briefly in place. */
  const handleCopy = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (!copyValue) return;
    navigator.clipboard.writeText(copyValue);
    setCopied(true);
    setTimeout(() => setCopied(false), 1800);
  };

  return (
    <div
      className={cn(
        'flex items-center justify-between gap-3 px-3.5 py-2.5 border-b border-emerald-ink/5 last:border-0 hover:bg-emerald-ink/[0.02] transition-colors',
        className
      )}
    >
      <span className="flex items-center gap-2 text-[12px] font-medium text-emerald-ink/70 shrink-0 select-none">
        {icon && <span className="text-emerald-ink/50">{icon}</span>}
        {label}
      </span>
      <div className="flex items-center gap-1.5 min-w-0 justify-end">
        <span className="text-[13px] text-emerald-ink font-medium truncate text-right">
          {children}
        </span>
        {copyValue && (
          <button
            type="button"
            onClick={handleCopy}
            title={`Copy ${label}`}
            className="p-1 rounded-md text-emerald-ink/40 hover:text-emerald-ink hover:bg-emerald-ink/10 transition-colors shrink-0"
          >
            {copied ? (
              <Check className="w-3.5 h-3.5 text-emerald-600 animate-in zoom-in-50 duration-150" />
            ) : (
              <Copy className="w-3.5 h-3.5" />
            )}
          </button>
        )}
      </div>
    </div>
  );
}
