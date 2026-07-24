import React from 'react';

/** A single label/value row inside one of the panel's info cards. */
export function InfoRow({
  icon,
  label,
  children,
}: {
  icon?: React.ReactNode;
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-3 p-3 border-b border-[#064E3B]/5 last:border-0">
      <span className="flex items-center gap-1.5 text-[13px] font-medium text-[#064E3B]/80 shrink-0">
        {icon}
        {label}
      </span>
      <span className="text-[13px] text-[#064E3B] font-medium truncate max-w-[200px] text-right">
        {children}
      </span>
    </div>
  );
}
