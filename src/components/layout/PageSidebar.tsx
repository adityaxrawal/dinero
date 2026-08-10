/**
 * Secondary sidebar used within individual pages.
 */
import React from 'react';
import { SidebarNavItem } from '@/components/ui/sidebar-nav-item';

export interface PageSidebarSection<T extends string> {
  id: T;
  label: string;
  icon: React.ElementType;
}

export interface PageSidebarProps<T extends string> {
  title: string;
  sections: readonly PageSidebarSection<T>[];
  currentSection: T;
  onSelectSection: (section: T) => void;
}

/** Secondary sidebar used within individual pages, generic over its section keys. */
export function PageSidebar<T extends string>({
  title,
  sections,
  currentSection,
  onSelectSection,
}: PageSidebarProps<T>) {
  return (
    <div
      className="flex-shrink-0 flex flex-col h-full border-r border-[#064E3B]/20"
      style={{ width: '320px', backgroundColor: 'var(--bg-canvas)' }}
    >
      <div className="flex flex-col gap-3 px-4 py-3 flex-shrink-0 border-b border-[#064E3B]/10">
        <h1 className="text-[14px] font-semibold text-[#064E3B] tracking-tight">{title}</h1>
      </div>

      <div className="flex-1 overflow-y-auto py-2">
        <nav className="flex flex-col gap-1">
          {sections.map((s) => {
            const isSelected = currentSection === s.id;
            return (
              <SidebarNavItem
                key={s.id}
                isSelected={isSelected}
                onClick={() => onSelectSection(s.id)}
                icon={<s.icon className="w-4 h-4" />}
                label={s.label}
              />
            );
          })}
        </nav>
      </div>
    </div>
  );
}
