/**
 * Primary navigation list for the sidebar.
 */
import SidebarItem from './SidebarItem';
import { buildNavGroups, useIsActive } from './navItems';
import { useUnprocessedStatements } from '@/hooks/queries/useUnprocessedStatements';

/** Primary navigation list for the sidebar. */
export default function SidebarNav({
  reviewCount,
  badgePulse,
}: {
  reviewCount: number;
  badgePulse: boolean;
}) {
  const isActive = useIsActive();
  // Statements waiting for the user get a badge, like review entries (audit
  // P3-L, channel rule "work waiting for you: queue + nav badge").
  const { data: waiting } = useUnprocessedStatements();
  const statementCount =
    (waiting?.awaiting_password.length ?? 0) +
    (waiting?.awaiting_instrument.length ?? 0) +
    (waiting?.awaiting_review?.length ?? 0);

  return (
    <nav
      className="flex-1 flex flex-col w-full py-4 gap-6 overflow-y-auto"
      aria-label="Primary navigation"
    >
      {buildNavGroups(reviewCount, badgePulse, statementCount).map((group) => (
        <div key={group.title} className="flex flex-col gap-1">
          <div className="px-6 text-[11px] font-semibold uppercase tracking-wider text-champagne/70 mb-1">
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
