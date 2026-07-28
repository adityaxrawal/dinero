import React, { useState } from 'react';
import { Copy, Check } from 'lucide-react';
import { cn } from '@/lib/utils';

/** A single label/value row inside one of the panel's info cards. */
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
        'flex items-center justify-between gap-3 px-3.5 py-2.5 border-b border-[#064E3B]/5 last:border-0 hover:bg-[#064E3B]/[0.02] transition-colors',
        className
      )}
    >
      <span className="flex items-center gap-2 text-[12px] font-medium text-[#064E3B]/70 shrink-0 select-none">
        {icon && <span className="text-[#064E3B]/50">{icon}</span>}
        {label}
      </span>
      <div className="flex items-center gap-1.5 min-w-0 justify-end">
        <span className="text-[13px] text-[#064E3B] font-medium truncate text-right">
          {children}
        </span>
        {copyValue && (
          <button
            type="button"
            onClick={handleCopy}
            title={`Copy ${label}`}
            className="p-1 rounded-md text-[#064E3B]/40 hover:text-[#064E3B] hover:bg-[#064E3B]/10 transition-colors shrink-0"
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

