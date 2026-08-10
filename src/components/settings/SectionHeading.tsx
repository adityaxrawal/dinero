/**
 * Consistent heading for a settings section.
 */
import type { LucideIcon } from 'lucide-react';
import { cn } from '@/lib/utils';

/** Consistent heading for a settings section. */
export default function SectionHeading({
  icon: Icon,
  title,
  description,
  iconClassName,
}: {
  icon: LucideIcon;
  title: string;
  description?: string;
  iconClassName?: string;
}) {
  return (
    <div className="mb-6">
      <h2 className="text-xl font-bold flex items-center gap-2">
        <Icon className={cn('w-5 h-5', iconClassName)} /> {title}
      </h2>
      {description && <p className="text-sm mt-1 text-[#064E3B]/70">{description}</p>}
    </div>
  );
}
