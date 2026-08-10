/**
 * One navigation item in the app sidebar.
 */
import { NavLink } from 'react-router-dom';
import { cn } from '@/lib/utils';

export interface NavItem {
  path: string;
  label: string;
  icon: React.ReactNode;
  badge?: number;
  pulse?: boolean;
}

/** Count badge on a navigation item, pulsing when newly incremented. */
function NavBadge({ item, isActive }: { item: NavItem; isActive: boolean }) {
  return (
    <span
      className={cn(
        'flex items-center justify-center text-[10px] font-bold rounded-full ml-auto px-2 min-w-[20px] h-5 transition-transform duration-300',
        isActive ? 'bg-[#064E3B]/10 text-[#064E3B]' : 'bg-[#f59e0b] text-white',
        item.pulse && 'scale-125'
      )}
      data-testid={item.pulse !== undefined ? 'reconciliation-badge' : undefined}
      data-pulsing={item.pulse}
      aria-hidden="true"
    >
      {item.badge! > 9 ? '9+' : item.badge}
    </span>
  );
}

/** One navigation item in the app sidebar. */
export default function SidebarItem({ item, isActive }: { item: NavItem; isActive: boolean }) {
  const hasBadge = item.badge != null && item.badge > 0;

  return (
    <NavLink
      to={item.path}
      end={item.path === '/'}
      className="relative block w-full px-3"
      aria-label={hasBadge ? `${item.label} — ${item.badge} pending` : item.label}
    >
      <span
        className={cn(
          'relative flex items-center rounded-lg transition-all px-3 py-1.5 w-full',
          isActive
            ? 'bg-[#F8E7C9] text-[#064E3B] font-semibold shadow-sm'
            : 'text-[#F8E7C9]/70 hover:text-[#F8E7C9] hover:bg-[#F8E7C9]/10 font-medium'
        )}
        aria-current={isActive ? 'page' : undefined}
      >
        <span className="flex-shrink-0 flex items-center justify-center w-6">{item.icon}</span>
        <span className="ml-2 text-[13px] whitespace-nowrap overflow-hidden">{item.label}</span>
        {hasBadge && <NavBadge item={item} isActive={isActive} />}
      </span>
    </NavLink>
  );
}
