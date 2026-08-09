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

/** `/` only matches exactly; every other route matches its subtree. */
export function useIsActive() {
  const location = useLocation();
  return (path: string) =>
    path === '/' ? location.pathname === '/' : location.pathname.startsWith(path);
}
