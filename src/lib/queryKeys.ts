/**
 * TASK-FE-003: centralized React Query key factory so invalidation
 * (`useIpcQueryInvalidation`) and query hooks always agree on the same key
 * shape — a key typo'd differently in a hook than in the invalidator would
 * silently invalidate nothing.
 */
export const queryKeys = {
  dashboard: {
    all: () => ['dashboard'] as const,
    summary: () => ['dashboard', 'summary'] as const,
    upcomingBills: () => ['dashboard', 'upcomingBills'] as const,
    categories: (month: string) => ['dashboard', 'categories', month] as const,
    spendTrend: (granularity: string) => ['dashboard', 'spendTrend', granularity] as const,
    pendingReview: () => ['dashboard', 'pendingReview'] as const,
  },
  transactions: {
    all: () => ['transactions'] as const,
    list: (page: number) => ['transactions', 'list', page] as const,
    detail: (id: string) => ['transactions', 'detail', id] as const,
    search: (query: string) => ['transactions', 'search', query] as const,
    observations: (id: string) => ['transactions', 'observations', id] as const,
    tags: (id: string) => ['transactions', 'tags', id] as const,
    emiGroup: (emiGroupId: string) => ['transactions', 'emiGroup', emiGroupId] as const,
  },
  instruments: {
    all: () => ['instruments'] as const,
    list: () => ['instruments', 'list'] as const,
  },
  statements: {
    all: () => ['statements'] as const,
    list: (page: number) => ['statements', 'list', page] as const,
    entries: (statementId: string) => ['statements', 'entries', statementId] as const,
  },
  reconciliation: {
    all: () => ['reconciliation'] as const,
    unresolved: () => ['reconciliation', 'unresolved'] as const,
  },
  tags: {
    all: () => ['tags'] as const,
    list: () => ['tags', 'list'] as const,
  },
} as const;
