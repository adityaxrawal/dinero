/* eslint-disable @typescript-eslint/no-explicit-any */
import { invoke } from '@tauri-apps/api/core';
import { readFile } from '@tauri-apps/plugin-fs';
import type { AppError } from '@/types/ipc';

/**
 * A typed wrapper around Tauri's invoke function.
 * Ensures strict typing of arguments and correctly throws structured AppErrors.
 *
 * Exported (TASK-SETUP-013) so `useIpcInvoke` can reuse the same
 * normalization logic instead of duplicating it.
 */
export async function invokeCommand<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    if (
      typeof error === 'object' &&
      error !== null &&
      'code' in error &&
      'message' in error
    ) {
      throw error as AppError;
    }
    throw {
      code: 'UNKNOWN_ERROR',
      message: typeof error === 'string' ? error : 'An unknown error occurred.',
      details: { original: error },
    } as AppError;
  }
}

export interface DashboardSummary {
  month_to_date_spend: number;
  limit: number;
  utilization_pct: number;
  recent_transactions_count: number;
  upcoming_bills_count: number;
  income: number;
}

// TASK-FE-008: types for the TASK-API-006 commands that had no frontend
// call site until now (src-tauri/src/commands/data.rs's exact struct shapes).
export interface UpcomingBill {
  id: string;
  description: string;
  amount: number;
  currency: string;
  due_date: string;
}

export interface CategorySpend {
  category_id: string;
  name: string;
  total_spend: number;
  monthly_budget: number | null;
  utilization_pct: number;
  currency: string;
}

export interface SpendTrendPoint {
  period: string;
  total_spend: number;
}

export type SpendTrendGranularity = 'daily' | 'weekly' | 'monthly';

export interface PendingReviewMetric {
  count: number;
  amount_minor: number;
}

// TASK-FE-010: matches src-tauri's TransactionsRow/TransactionObservationsRow/
// MatchDecisionsRow/TransactionDetail/EmiGroupSummary exactly (transactions_get
// and transactions_get_emi_group's real response shapes — was typed `any`
// before this task, with no frontend consumer to have caught a mismatch).
export interface CanonicalTransaction {
  id: string;
  unique_event_id: string | null;
  instrument_id: string | null;
  instrument_type: string | null;
  direction: string | null;
  amount: number | null;
  amount_minor: number | null;
  currency: string | null;
  authorization_time: string | null;
  best_event_time: string | null;
  event_time_confidence: string | null;
  best_posting_date: string | null;
  posting_date_confidence: string | null;
  merchant_display_name: string | null;
  merchant_normalized_name: string | null;
  merchant_entity_id: string | null;
  reference_id: string | null;
  location: string | null;
  original_amount_minor: number | null;
  original_currency: string | null;
  exchange_rate: number | null;
  balance_after_transaction: number | null;
  status: string | null;
  match_confidence: string | null;
  source_mix: string | null;
  alert_fired: boolean | null;
  parent_transaction_id: string | null;
  transaction_subtype: string | null;
  emi_group_id: string | null;
  category_id: string | null;
  is_deleted: boolean;
  created_at: string | null;
  updated_at: string | null;
  notes: string | null;
}

export interface TransactionObservation {
  id: string;
  canonical_transaction_id: string | null;
  source_pipeline: string | null;
  source_record_id: string | null;
  source_message_id: string | null;
  source_thread_id: string | null;
  statement_id: string | null;
  statement_entry_id: string | null;
  instrument_id: string | null;
  direction: string | null;
  amount: number | null;
  amount_minor: number | null;
  currency: string | null;
  event_time: string | null;
  event_time_confidence: string | null;
  posting_date: string | null;
  merchant_raw: string | null;
  merchant_normalized: string | null;
  reference_id: string | null;
  original_amount_minor: number | null;
  original_currency: string | null;
  exchange_rate: number | null;
  balance_after_transaction: number | null;
  timezone_at_ingestion: string | null;
  fingerprint: string | null;
  extraction_method: string | null;
  confidence_score: number | null;
  raw_payload_json: string | null;
  parser_version: string | null;
  emi_total_installments: number | null;
  emi_installment_number: number | null;
  emi_original_amount_minor: number | null;
  is_deleted: boolean;
  created_at: string | null;
  updated_at: string | null;
}

export interface MatchDecision {
  id: string;
  observation_id: string | null;
  matched_transaction_id: string | null;
  decision: string | null;
  score: number | null;
  rules_triggered_json: string | null;
  review_status: string | null;
  reviewed_by: string | null;
  created_at: string | null;
}

export interface TransactionDetailResponse {
  transaction: CanonicalTransaction;
  observations: TransactionObservation[];
  match_decisions: MatchDecision[];
}

export interface EmiInstallmentDetail {
  transaction_id: string;
  amount_minor: number;
  event_time: string | null;
}

export interface EmiGroupSummary {
  installments_paid: number;
  total_paid_minor: number;
  total_installments: number | null;
  installments: EmiInstallmentDetail[];
}

export interface UnprocessedStatementEntry {
  statement_id: string;
  filename: string;
  failure_type: string | null;
  failure_reason: string | null;
}

export interface UnprocessedStatementGroups {
  awaiting_password: UnprocessedStatementEntry[];
  pending_retry: UnprocessedStatementEntry[];
  failed: UnprocessedStatementEntry[];
}

export interface CategoryRecord {
  id: string;
  parent_id: string | null;
  name: string;
  source_type: string;
  mcc_code: string | null;
  monthly_budget_minor: number | null;
  is_deleted: boolean;
  created_at: string | null;
  color: string | null;
  icon: string | null;
}

export interface TagRecord {
  id: string;
  name: string;
  color_hex: string | null;
  created_at: string | null;
}

export interface TransactionRecord {
  id: string;
  date: string;
  merchant: string;
  amount: number;
  category: string;
  status: string;
  tags?: string[];
  source_mix: string | null;
  instrument_id: string | null;
}

export interface TransactionsPage {
  records: TransactionRecord[];
  total: number;
}

// TASK-FE-009: matches src-tauri/src/commands/data.rs's TransactionListFilters
// exactly (Document 19 §8.1's documented combinable filter args). Doc30's
// task text also names "amount"/"tags" as filter dimensions, but neither is
// part of the real, documented contract -- not built here, consistent with
// this session's established precedent (Doc19/18 naming and scope win over
// Doc30 prose).
export interface TransactionListFilters {
  from_date?: string;
  to_date?: string;
  instrument_id?: string;
  direction?: 'debit' | 'credit';
  category_id?: string;
  status?: string;
}

export interface StatementRecord {
  id: string;
  date: string;
  file_name: string;
  status: string;
  instrument_id: string | null;
}

// TASK-FE-013: mirrors the real Rust `ClusterMember` shape (Document 18
// §4.6a) -- `member_role` distinguishes the triggering "incoming"
// observation from its "candidate_*" matches, and `observation_id` is the
// real id `reconciliation_clusters_resolve` requires (not `id`, which is
// only the join-table row's own key).
export type ClusterMemberRole = 'incoming' | 'candidate_a' | 'candidate_b' | 'candidate_other';

export interface ClusterMember {
  id: string;
  member_role: ClusterMemberRole;
  observation_id: string | null;
  canonical_transaction_id: string | null;
  source_pipeline: string | null;
  merchant: string;
  amount: number;
  date: string;
}

export interface ClusterRecord {
  id: string;
  reason: string;
  members_count: number;
  members: ClusterMember[];
}

export interface UnassignedTransactionRecord {
  id: string;
  observation_id: string;
  reason: string;
  status: string;
  created_at: string | null;
}

export interface InstrumentRecord {
  id: string;
  instrument_type: string;
  issuer_name: string;
  masked_identifier: string;
  status: string;
  current_balance?: number;
  credit_limit?: number;
  full_identifier?: string;
  billing_cycle_day?: number;
  bank_ifsc?: string;
}

export interface DebugMetrics {
  total_transactions: number;
  total_statements: number;
  unresolved_clusters: number;
  db_size_bytes: number;
  app_version: string;
  llm_fallback_rate: number;
  queue_depth: number;
  extraction_layer_distribution: Record<string, number>;
  reconciliation_decision_distribution: Record<string, number>;
}

interface BackendStatus {
  status: 'healthy' | 'corrupted' | 'locked';
}

// TASK-FE-015: account_status added -- the real backend column existed
// (Document 18's connected_accounts.account_status) but was never selected
// or exposed, so a 'degraded' account (Gmail token refresh failed) had no
// way to surface a status badge.
export interface ConnectedAccountInfo {
  email: string;
  account_id: string;
  account_status: string;
}

export interface CategoryBudget {
  name: string;
  budget: number;
}

// Doc 16 §12.3's 5-tier local LLM hardware matrix — mirrors
// src-tauri/src/llm_manager.rs::LlmModelInfo exactly.
export interface LlmModelInfo {
  id: string;
  name: string;
  tag: string;
  tier: number;
  min_ram_gb: number;
  approx_size_gb: number;
  rationale: string;
  gguf_url: string;
  expected_sha256: string;
  tokenizer_url: string | null;
}

export interface SpendingLimits {
  global_limit: number;
  thresholds: {
    warn_at_80: boolean;
    warn_at_90: boolean;
    warn_at_100: boolean;
  };
  categories: CategoryBudget[];
}

// Scaffold examples for Phase 5 frontend
export const API = {
  dashboard: {
    getSummary: () =>
      invokeCommand<DashboardSummary>('dashboard_summary'),
    // TASK-API-006 built these with no frontend call site; wired here.
    getUpcomingBills: () =>
      invokeCommand<{ bills: UpcomingBill[] }>('dashboard_upcoming_bills').then((r) => r.bills),
    getCategories: (month: string) =>
      invokeCommand<{ categories: CategorySpend[] }>('dashboard_categories', { month }).then((r) => r.categories),
  },
  analytics: {
    getSpendTrend: (granularity: SpendTrendGranularity) =>
      invokeCommand<SpendTrendPoint[]>('analytics_spend_trend', { granularity }),
    getPendingReviewCount: () =>
      invokeCommand<PendingReviewMetric>('analytics_pending_review_count'),
  },
  transactions: {
    // G9 fix: real offset-based pagination — `page` is honored server-side
    // and the response carries the real total row count.
    // G20/H10/J8 fix: renamed to match Doc 19 §8.1's documented `transactions_list`.
    list: (page = 1, filters?: TransactionListFilters) =>
      invokeCommand<TransactionsPage>('transactions_list', { page, filters }),
    search: (query: string) => invokeCommand<TransactionRecord[]>('transactions_search', { query }),
    // G12 fix: transaction_create/transaction_delete existed on the backend
    // but had no ipc.ts wrapper or UI at all. G20/H10/J8 fix: both renamed to
    // the plural `transactions_*` form to match Doc 19 §8.4/§8.5.
    create: (input: {
      amountMinor: number;
      currency: string;
      direction: 'credit' | 'debit';
      eventTime: string;
      merchantName: string;
      instrumentId: string;
    }) =>
      invokeCommand<string>('transactions_create', {
        payload: {
          amount_minor: input.amountMinor,
          currency: input.currency,
          direction: input.direction,
          event_time: input.eventTime,
          merchant_name: input.merchantName,
          instrument_id: input.instrumentId,
        },
      }),
    delete: (transactionId: string) => invokeCommand<string>('transactions_delete', { transactionId }),
    // Doc 30 TASK-API-003: matches the real `TransactionUpdatePayload`
    // contract (renamed from `ManualTransactionUpdatePayload` -- this
    // command was never actually restricted to manual transactions).
    // Editable fields per Document 19 §8.3 exactly: merchant_display_name,
    // category_id, notes, location (amount/currency/direction/event_time
    // were removed from the backend entirely -- evidence-derived fields,
    // never actually sent by this wrapper even before the fix). `tags`
    // remains as an additive bulk-replace convenience, not itself in
    // Document 19's editable-field list.
    update: (
      transactionId: string,
      updates: { merchantDisplayName?: string; categoryId?: string; notes?: string; location?: string; tags?: string[] },
    ) =>
      invokeCommand<string>('transactions_update', {
        payload: {
          transaction_id: transactionId,
          merchant_display_name: updates.merchantDisplayName,
          category_id: updates.categoryId,
          notes: updates.notes,
          location: updates.location,
          tags: updates.tags,
        },
      }),
    get: (id: string) => invokeCommand<TransactionDetailResponse>('transactions_get', { id }),
    getObservations: (id: string) =>
      invokeCommand<TransactionObservation[]>('fetch_transaction_observations', { transactionId: id }),
    getSourceLog: (id: string) => invokeCommand<string>('fetch_transaction_source_log', { transactionId: id }),
    // G13 fix: the tag names currently on a transaction (relational, not free-text).
    getTags: (id: string) => invokeCommand<string[]>('fetch_transaction_tags', { transactionId: id }),
    addTag: (transactionId: string, tagId: string) =>
      invokeCommand<string>('transactions_add_tag', { transactionId, tagId }),
    removeTag: (transactionId: string, tagId: string) =>
      invokeCommand<string>('transactions_remove_tag', { transactionId, tagId }),
    getEmiGroup: (emiGroupId: string) =>
      invokeCommand<EmiGroupSummary>('transactions_get_emi_group', { emiGroupId }),
  },
  tags: {
    // G13 fix: the full reusable-tag catalog, for autocomplete. Returns
    // full rows (id + name) so callers can resolve a name to the id
    // `transactions_add_tag`/`_remove_tag` (Doc19 §8.7/§8.8) require.
    list: () => invokeCommand<TagRecord[]>('tags_list'),
    create: (name: string) => invokeCommand<{ id: string; status: string }>('tags_create', { payload: { name } }),
  },
  categories: {
    // TASK-API-007 built categories_list with no frontend call site at all
    // until now (TASK-FE-009 needs a real category filter dropdown).
    list: () => invokeCommand<CategoryRecord[]>('categories_list'),
  },
  statements: {
    // H2 fix: statements_upload is now a real multi-file batch contract
    // (Doc 19 §9.1, FR-031) — one IPC call processes every selected file.
    // Doc 30 TASK-API-004 fix: the real backend command needs `files:
    // [{ file_bytes, filename }]` (actual PDF byte content), not paths —
    // the previous `{ filePaths }` shape never matched the command's real
    // argument type, so upload from the UI never actually worked. Reads
    // each dialog-selected path's bytes via the newly-added, read-only,
    // capability-scoped `@tauri-apps/plugin-fs` (tauri.conf.json's
    // `fs:allow-read-file`) before sending — the bytes are held in memory
    // only for this one IPC call, consistent with Document 15's "raw PDFs
    // never persisted" invariant (this is the frontend's read of the
    // user's own already-selected file, not a new persistence path).
    upload: async (filePaths: string[]) => {
      const files = await Promise.all(
        filePaths.map(async (path) => {
          const bytes = await readFile(path);
          const filename = path.split(/[/\\]/).pop() || path;
          return { file_bytes: Array.from(bytes), filename };
        }),
      );
      // Doc 30 TASK-API-004 fix: the real command wraps its array in
      // `{ results: [...] }` (Document 19 §9.1's exact response shape) --
      // this wrapper was previously also mismatched (the caller expected a
      // bare array), which would have thrown ("not iterable") the moment
      // upload actually reached a real response.
      const response = await invokeCommand<{
        results: Array<{ status: string; statement_id?: string; filename?: string; error?: string }>;
      }>('statements_upload', { files });
      return response.results;
    },
    submitPassword: (
      statementId: string,
      instrumentId: string,
      password: string,
    ) =>
      invokeCommand<{ status: string; statement_id: string; attempts_remaining?: number }>(
        'statements_submit_password',
        { statementId, instrumentId, password },
      ),
    confirmInstrument: (
      statementId: string,
      issuerName: string,
      maskedIdentifier: string,
      instrumentType: string,
    ) =>
      invokeCommand<{ status: string; statement_id: string }>('statements_confirm_instrument', {
        statementId,
        issuerName,
        maskedIdentifier,
        instrumentType,
      }),
    // TASK-FE-012: TASK-STMT-010 built this dedicated unprocessed-items
    // retry/recovery backend (statements_list_unprocessed/_retry_unprocessed/
    // _discard) with zero frontend call sites -- the pre-existing page
    // instead derived its own ad-hoc "unprocessed" list by filtering the
    // general statement history for PASSWORD_REQUIRED/FAILED, missing the
    // real 3-bucket grouping (awaiting_password/pending_retry/failed) and
    // the discard action entirely.
    listUnprocessed: () =>
      invokeCommand<UnprocessedStatementGroups>('statements_list_unprocessed'),
    retryUnprocessed: (statementId: string) =>
      invokeCommand<{ status: string; statement_id: string }>('statements_retry_unprocessed', { statementId }),
    discard: (statementId: string) =>
      invokeCommand<{ status: string }>('statements_discard', { statementId }),
    // Doc 30 TASK-API-004: `statements_list` now returns a real paginated
    // page ({ records, total }, matching Document 19 §3.3's pagination
    // convention -- it previously had no pagination at all). Unwrapped to
    // `.records` here so the existing array-shaped consumer keeps working;
    // `page`/`total` are available for a future paginated statements UI.
    listHistory: (page = 1) =>
      invokeCommand<{ records: StatementRecord[]; total: number }>('statements_list', { page })
        .then((res) => res.records),
    getEntries: (statementId: string) =>
      invokeCommand<any[]>('statements_get_entries', { statementId }),
  },
  reconciliation: {
    // G20/H10/J8 fix: both renamed to match Doc 19 §10.1/§10.3's documented
    // `reconciliation_clusters_*` naming family.
    listUnresolved: () =>
      invokeCommand<ClusterRecord[]>('reconciliation_clusters_list'),
    // TASK-FE-013: `reconciliation_clusters_get` (Doc 19 §10.2) existed on
    // the backend with zero frontend call site -- needed for the cluster
    // detail page.
    getCluster: (clusterId: string) =>
      invokeCommand<ClusterRecord>('reconciliation_clusters_get', { clusterId }),
    // TASK-FE-013: `reconciliation_get_unassigned_transactions` (TASK-API-005)
    // existed on the backend with zero frontend call site -- extraction
    // failures (no instrument resolved) are a queue distinct from ambiguous
    // clusters (matching ambiguity) and need their own UI section.
    listUnassigned: () =>
      invokeCommand<UnassignedTransactionRecord[]>('reconciliation_get_unassigned_transactions'),
    // G20/H10/J8 fix: action vocabulary realigned to Doc 19 §10.3's
    // documented "Allowed actions" ('confirm_match'/'reject_candidate'
    // replace the previous 'merge'/'reject' — 'keep_separate' and
    // 'mark_unresolved' already matched).
    // TASK-FE-013 fix: the real `reconciliation_clusters_resolve` command
    // requires `observation_id` (Document 12's `resolve_cluster` reads it
    // for every action except `mark_unresolved`) -- this wrapper never sent
    // it at all, so every resolve call was missing a required argument.
    resolve: (
      clusterId: string,
      observationId: string,
      action: 'confirm_match' | 'reject_candidate' | 'keep_separate' | 'mark_unresolved',
      chosenCanonicalId?: string,
    ) =>
      invokeCommand<string>('reconciliation_clusters_resolve', {
        clusterId,
        observationId,
        action,
        chosenCanonicalId,
      }),
  },
  instruments: {
    list: () => invokeCommand<InstrumentRecord[]>('instruments_list'),
    create: (instrumentType: string, issuerName: string, maskedIdentifier: string, fullIdentifier?: string, billingCycleDay?: number, bankIfsc?: string) =>
      invokeCommand<InstrumentRecord>('instruments_create', {
        payload: {
          instrument_type: instrumentType,
          issuer_name: issuerName,
          masked_identifier: maskedIdentifier,
          full_identifier: fullIdentifier,
          billing_cycle_day: billingCycleDay,
          bank_ifsc: bankIfsc,
        }
      }),
    // Doc 30 TASK-API-002: issuer_name/masked_identifier are identity
    // fields (used by resolve_instrument()'s matching key, Document 15
    // §2.8) and are never editable post-creation -- the backend's
    // InstrumentUpdatePayload no longer even has fields for them, so this
    // wrapper no longer accepts them either.
    update: (id: string, fullIdentifier?: string, billingCycleDay?: number, bankIfsc?: string) =>
      invokeCommand<string>('instruments_update', {
        payload: {
          id,
          full_identifier: fullIdentifier,
          billing_cycle_day: billingCycleDay,
          bank_ifsc: bankIfsc,
        }
      }),
    get: (id: string) => invokeCommand<InstrumentRecord>('instruments_get', { id }),
    delete: (id: string) =>
      invokeCommand<string>('instruments_archive', { id }),
  },
  spendingLimits: {
    get: () => invokeCommand<SpendingLimits>('fetch_spending_limits'),
    update: (limits: SpendingLimits) =>
      invokeCommand<string>('update_spending_limits', { limits }),
  },
  onboarding: {
    // G19 fix: previously onboarding choices were only written to browser
    // localStorage — never persisted to `local_profile`, so they didn't
    // survive a reinstall/reset and never reached the same row Settings →
    // Spending Limits reads from. Nested struct fields use exact snake_case
    // (only top-level invoke args get camelCase conversion).
    savePreferences: (prefs: {
      timezone: string;
      spendingLimitMonthly: number;
      historicalScanMonths: number;
      llmModel: string;
      statementPreference: string;
    }) =>
      invokeCommand<string>('onboarding_save_preferences', {
        preferences: {
          timezone: prefs.timezone,
          spending_limit_monthly: prefs.spendingLimitMonthly,
          historical_scan_months: prefs.historicalScanMonths,
          llm_model: prefs.llmModel,
          statement_preference: prefs.statementPreference,
        },
      }),
  },
  status: {
    check: () => invokeCommand<BackendStatus>('check_backend_status'),
  },
  db: {
    restoreBackup: () => invokeCommand<string>('db_restore_backup'),
    // J7 fix: local encrypted export of the full dataset — previously no
    // such command existed at all. `password`, when provided, additionally
    // AES-256-GCM-encrypts the export with that password (portable to a
    // different Mac) instead of relying solely on this machine's Keychain-
    // derived SQLCipher key.
    exportData: (exportPath: string, password?: string) =>
      invokeCommand<string>('settings_export_data', { exportPath, password: password ?? null }),
  },
  dev: {
    resetDatabase: () => invokeCommand<string>('settings_delete_account'),
    getMetrics: () => invokeCommand<DebugMetrics>('get_debug_metrics'),
    checkSystemRam: () => invokeCommand<number>('check_system_ram'),
  },
  support: {
    // Doc 19 §21.1: generates the privacy-safe diagnostic bundle.
    exportLogs: () => invokeCommand<{ success: boolean; file_path: string }>('export_logs'),
    // Doc 41 §5: attaches the same bundle when include_logs is true.
    submitFeedback: (text: string, includeLogs: boolean) =>
      invokeCommand<string>('submit_user_feedback', { text, includeLogs }),
  },
  auth: {
    // TASK-DB-022: the backend resolves the single local profile internally
    // rather than trusting a caller-supplied profileId (Document 22 §13.1).
    startGoogle: () => invokeCommand<string>('auth_google_start'),
    isGmailConnected: () => invokeCommand<boolean>('is_gmail_connected'),
    // Doc 03 §8.2: a license supports up to 10 simultaneously connected
    // Gmail accounts — the list can have more than one entry.
    listConnectedAccounts: () =>
      invokeCommand<ConnectedAccountInfo[]>('settings_get_connected_accounts'),
    disconnectGmail: (accountId: string) =>
      invokeCommand<void>('auth_google_disconnect', { accountId }),
    // Doc 19 §5.4, Doc 22 §8.2: opt-in 24-word Secure Backup Recovery Phrase.
    getRecoveryPhrase: () => invokeCommand<string>('auth_get_recovery_phrase'),
  },
  ingestion: {
    startHistoricalScan: (accountId: string, startDate: string, endDate: string) =>
      invokeCommand<string>('scans_historical', { accountId, startDate, endDate }),
  },
  llm: {
    // Doc 16 §12.3: the single source of truth for the 5-tier model catalog —
    // frontend selectors must render this, not a hardcoded local list.
    getAvailableModels: () => invokeCommand<LlmModelInfo[]>('llm_get_available_models'),
    downloadModel: (modelId: string) =>
      invokeCommand<void>('llm_download_model', { modelId }),
  },
  debug: {
    fetchParseErrors: () => invokeCommand<any[]>('debug_fetch_parse_errors'),
    fetchUnprocessedStatements: () => invokeCommand<any[]>('debug_fetch_unprocessed_statements'),
    fetchAuditLog: (resourceTypeFilter?: string, limit: number = 50, offset: number = 0) =>
      invokeCommand<any[]>('debug_fetch_audit_log', { resourceTypeFilter, limit, offset }),
    fetchPatternRuleHealth: () => invokeCommand<any[]>('debug_fetch_pattern_rule_health'),
    fetchReconciliationClusters: () => invokeCommand<any[]>('debug_fetch_reconciliation_clusters'),
    getPipelineState: () => invokeCommand<any>('debug_get_pipeline_state'),
    setGmailPollPaused: (paused: boolean) => invokeCommand<void>('debug_set_gmail_poll_paused', { paused }),
    setScanQueuePaused: (paused: boolean) => invokeCommand<void>('debug_set_scan_queue_paused', { paused }),
  },
  network: {
    getActivityList: () =>
      invokeCommand<{ entries: any[] }>('settings_get_network_activity').then((r) => r.entries),
  },
  patternRules: {
    // G14 fix: a real Settings-facing toggle, not just a Debug-only view.
    // TASK-API-008: now backed by the real settings_pattern_rules_list
    // command (previously borrowed the debug-only health view as a
    // stopgap).
    list: () => invokeCommand<PatternRuleHealth[]>('settings_pattern_rules_list'),
    setStatus: (ruleId: string, newStatus: 'active' | 'inactive' | 'flagged') =>
      invokeCommand<void>('settings_pattern_rules_update', { ruleId, newStatus }),
  },
  pdfPasswords: {
    // G15 fix: management UI for stored PDF passwords (metadata only — the
    // password itself is never sent to the frontend).
    list: () => invokeCommand<PdfPasswordSummary[]>('settings_pdf_passwords_list'),
    delete: (id: string) => invokeCommand<void>('settings_pdf_passwords_delete', { id }),
  },
  privacy: {
    // TASK-AUTH-003, Doc 19 §5.6: Settings → Privacy → Consent History,
    // backed by the dedicated consent_events table (Doc 18 §4.21a).
    getConsentHistory: (limit = 50, offset = 0) =>
      invokeCommand<ConsentEventRecord[]>('auth_get_consent_history', { limit, offset }),
    recordConsentEvent: (consentType: string, detail: string) =>
      invokeCommand<void>('record_consent_event', { consentType, detail }),
  },
  licensing: {
    // Doc 19 §14.1
    getStatus: () => invokeCommand<LicenseStatusResponse>('license_get_status'),
    // Doc 19 §14.2: Razorpay payment confirmation, bound to this device — no
    // separate license-key concept (C10).
    activate: (email: string, razorpayPaymentId: string, razorpaySignature: string, billingInterval: string) =>
      invokeCommand<LicenseActivateResponse>('license_activate', {
        email,
        razorpayPaymentId,
        razorpaySignature,
        billingInterval,
      }),
    // Doc 19 §14.3
    deactivate: () => invokeCommand<LicenseDeactivateResponse>('license_deactivate'),
    // Doc 19 §14.4 — same action the C11-fixed background loop calls every 6h.
    refresh: () => invokeCommand<LicenseRefreshResponse>('license_refresh'),
  },
  updater: {
    // Doc 30 TASK-DESK-005: not in Document 19's catalog (this task
    // predates/extends it, same precedent as several Area 8 additive
    // commands) -- installs whichever update the most recent check found.
    confirmInstall: () => invokeCommand<void>('updater_confirm_install'),
  },
};

export interface LicenseStatusResponse {
  state: string;
  is_active: boolean;
  license_key_masked: string | null;
  plan_id: string | null;
  billing_interval: string | null;
  expiry_date: string | null;
  days_remaining: number | null;
}

export interface LicenseActivateResponse {
  status: string;
  state: string;
  device_bound: boolean;
  plan_id: string | null;
  billing_interval: string | null;
  expires_at: string | null;
}

export interface LicenseDeactivateResponse {
  status: string;
  state: string;
}

export interface LicenseRefreshResponse {
  status: string;
  state: string;
}

export interface PatternRuleHealth {
  id: string;
  merchant_id: string;
  pattern_type: string;
  pattern_value: string;
  is_active: boolean;
  success_count: number;
  failure_count: number;
}

export interface PdfPasswordSummary {
  id: string;
  instrument_id: string;
  issuer_name: string;
  masked_identifier: string;
  success_count: number;
  last_used_at: string | null;
}

export interface ConsentEventRecord {
  id: string;
  event_type: string;
  disclosure_text: string;
  consented_at: string;
  withdrawn_at: string | null;
}

export interface ScanProgressPayload {
  account_id: string;
  processed: number;
  total: number;
  transactions_found: number;
  statements_found: number;
  non_financial: number;
  errors: number;
  error_message?: string;
}
