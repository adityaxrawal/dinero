import { createHashRouter, Navigate } from 'react-router-dom';
import AppLayout from '@/components/layout/AppLayout';
import Dashboard from '@/pages/Dashboard';
import Transactions from '@/pages/Transactions';
import TransactionDetail from '@/pages/TransactionDetail';
import Instruments from '@/pages/Instruments';
import InstrumentDetail from '@/pages/InstrumentDetail';
import Statements from '@/pages/Statements';
import Reconciliation from '@/pages/Reconciliation';
import ReconciliationClusterDetail from '@/pages/ReconciliationClusterDetail';
import Settings from '@/pages/Settings';
import Debug from '@/pages/Debug';
import OnboardingFlow from './onboarding/OnboardingFlow';
import SpendingLimits from '@/pages/SpendingLimits';

/**
 * Application route table.
 *
 * A hash router rather than a browser router: the app is served from the local
 * filesystem inside a Tauri webview, where there is no HTTP server to rewrite
 * deep paths, so history-based routing would break on reload.
 *
 * Two top-level branches. Onboarding sits outside the shell because it runs
 * before there is an account to navigate around; everything else nests under
 * AppLayout, which supplies the persistent sidebar and mounts each screen into
 * its Outlet.
 */
export const router = createHashRouter([
  // Standalone: no sidebar, no navigation away until the flow completes.
  { path: '/onboarding', element: <OnboardingFlow /> },
  {
    path: '/',
    element: <AppLayout />,
    children: [
      { index: true, element: <Dashboard /> },
      { path: 'transactions', element: <Transactions /> },
      { path: 'transactions/:id', element: <TransactionDetail /> },
      { path: 'instruments', element: <Instruments /> },
      { path: 'instruments/:id', element: <InstrumentDetail /> },
      { path: 'statements', element: <Statements /> },
      { path: 'reconciliation', element: <Reconciliation /> },
      { path: 'reconciliation/:clusterId', element: <ReconciliationClusterDetail /> },
      { path: 'spending-limits', element: <SpendingLimits /> },
      { path: 'settings', element: <Settings /> },
      // Debug screen is spliced in only for development builds, so the route
      // does not exist at all in a shipped binary.
      ...(import.meta.env.DEV ? [{ path: 'debug', element: <Debug /> }] : []),
      // Catch-all: unknown paths redirect to the dashboard rather than showing
      // an error. `replace` keeps the bad URL out of history, so Back does not
      // return to it.
      { path: '*', element: <Navigate to="/" replace /> },
    ],
  },
]);
