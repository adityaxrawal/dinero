/**
 * Central registry of React Query cache keys, grouped by data domain.
 *
 * Every key is produced by a function here rather than written inline at the
 * call site. That matters because invalidation depends on exact array equality:
 * a hand-written key that differs by even one element silently creates a second
 * cache entry, and the stale one never gets refreshed.
 *
 * The arrays are hierarchical and ordered broad-to-narrow, which is what makes
 * partial invalidation work -- invalidating `transactions.all()` also clears
 * every list, detail and search entry beneath it, because React Query matches
 * keys by prefix. Each group therefore exposes an `all()` root alongside its
 * specific entries, and parameterised keys append their arguments last.
 *
 * `as const` throughout preserves the literal tuple types, so a typo in a key
 * is a compile error rather than a cache miss discovered at runtime.
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
    infiniteList: (filters: object) => ['transactions', 'infiniteList', filters] as const,
    detail: (id: string) => ['transactions', 'detail', id] as const,
    search: (query: string, filters?: object) => ['transactions', 'search', query, filters] as const,
    observations: (id: string) => ['transactions', 'observations', id] as const,
    tags: (id: string) => ['transactions', 'tags', id] as const,
    emiGroup: (emiGroupId: string) => ['transactions', 'emiGroup', emiGroupId] as const,
    sourceLog: (id: string) => ['transactions', 'sourceLog', id] as const,
  },
  instruments: {
    all: () => ['instruments'] as const,
    list: () => ['instruments', 'list'] as const,
    detail: (id: string) => ['instruments', 'detail', id] as const,
  },
  statements: {
    all: () => ['statements'] as const,
    list: () => ['statements', 'list'] as const,
    entries: (statementId: string) => ['statements', 'entries', statementId] as const,
    unprocessed: () => ['statements', 'unprocessed'] as const,
  },
  learnedRules: {
    all: () => ['learnedRules'] as const,
    list: () => ['learnedRules', 'list'] as const,
  },
  senderOverrides: {
    all: () => ['senderOverrides'] as const,
    list: () => ['senderOverrides', 'list'] as const,
  },
  pdfPasswords: {
    all: () => ['pdfPasswords'] as const,
    list: () => ['pdfPasswords', 'list'] as const,
  },
  reconciliation: {
    all: () => ['reconciliation'] as const,
    unresolved: () => ['reconciliation', 'unresolved'] as const,
    cluster: (clusterId: string) => ['reconciliation', 'cluster', clusterId] as const,
    unassigned: () => ['reconciliation', 'unassigned'] as const,
  },
  tags: {
    all: () => ['tags'] as const,
    list: () => ['tags', 'list'] as const,
  },
  categories: {
    all: () => ['categories'] as const,
    list: () => ['categories', 'list'] as const,
  },
} as const;
