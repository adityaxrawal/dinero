/**
 * Primary navigation list for the sidebar.
 */
import SidebarItem from './SidebarItem';
import { buildNavGroups, useIsActive } from './navItems';

/** Primary navigation list for the sidebar. */
export default function SidebarNav({
  reviewCount,
  badgePulse,
}: {
  reviewCount: number;
  badgePulse: boolean;
}) {
  const isActive = useIsActive();

  return (
    <nav
      className="flex-1 flex flex-col w-full py-4 gap-6 overflow-y-auto"
      aria-label="Primary navigation"
    >
      {buildNavGroups(reviewCount, badgePulse).map((group) => (
        <div key={group.title} className="flex flex-col gap-1">
          <div className="px-6 text-[10px] font-semibold uppercase tracking-wider text-champagne/40 mb-1">
            {group.title}
          </div>
          {group.items.map((item) => (
            <SidebarItem key={item.path} item={item} isActive={isActive(item.path)} />
          ))}
        </div>
      ))}
    </nav>
  );
}
