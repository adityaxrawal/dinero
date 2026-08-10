/**
 * Declares the navigation entries and the active-route matcher.
 *
 * Structure as data, so adding a screen means adding an entry rather than
 * editing layout markup.
 */
import { useLocation } from 'react-router-dom';
import {
  LayoutDashboard,
  ArrowLeftRight,
  CreditCard,
  FileText,
  GitMerge,
  Settings,
  Activity,
} from 'lucide-react';
import type { NavItem } from './SidebarItem';

/** Builds the navigation groups, including counts and dev-only entries. */
export function buildNavGroups(unresolvedClusters: number, badgePulse: boolean) {
  return [
    {
      title: 'Workspace',
      items: [
        { path: '/', label: 'Command Center', icon: <LayoutDashboard size={15} />, badge: 0 },
        {
          path: '/reconciliation',
          label: 'Review Inbox',
          icon: <GitMerge size={15} />,
          badge: unresolvedClusters,
          pulse: badgePulse,
        },
        { path: '/statements', label: 'Statements', icon: <FileText size={15} />, badge: 0 },
      ],
    },
    {
      title: 'Ledger',
      items: [
        {
          path: '/transactions',
          label: 'All Transactions',
          icon: <ArrowLeftRight size={15} />,
          badge: 0,
        },
        { path: '/instruments', label: 'Accounts', icon: <CreditCard size={15} />, badge: 0 },
      ],
    },
  ];
}

export const SYSTEM_ITEMS: NavItem[] = [
  { path: '/settings', label: 'Settings', icon: <Settings size={15} />, badge: 0 },
  ...(import.meta.env.DEV
    ? [{ path: '/debug', label: 'Debug', icon: <Activity size={15} />, badge: 0 }]
    : []),
];

/**
 * Returns a predicate for whether a route is currently active.
 *
 * Prefix matching, so a detail route keeps its parent nav item highlighted.
 */
export function useIsActive() {
  const location = useLocation();
  return (path: string) =>
    path === '/' ? location.pathname === '/' : location.pathname.startsWith(path);
}
