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

// Document 30 TASK-FE-001: hash-based routing (`createHashRouter`), not
// browser history routing — Tauri's `asset://` packaging has no server to
// fall back to index.html for an arbitrary deep-linked path, only a
// `#`-fragment survives that.
export const router = createHashRouter([
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
      // F14 fix: Debug Console must not be reachable in production builds.
      ...(import.meta.env.DEV ? [{ path: 'debug', element: <Debug /> }] : []),
      { path: '*', element: <Navigate to="/" replace /> },
    ],
  },
]);
