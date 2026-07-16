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

export interface TransactionRecord {
  id: string;
  date: string;
  merchant: string;
  amount: number;
  category: string;
  status: string;
  tags?: string[];
  source_mix: string | null;
}

export interface TransactionsPage {
  records: TransactionRecord[];
  total: number;
}

export interface StatementRecord {
  id: string;
  date: string;
  file_name: string;
  status: string;
}

export interface ClusterMember {
  id: string;
  source: string;
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

export interface ConnectedAccountInfo {
  email: string;
  account_id: string;
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
  },
  transactions: {
    // G9 fix: real offset-based pagination — `page` is honored server-side
    // and the response carries the real total row count.
    // G20/H10/J8 fix: renamed to match Doc 19 §8.1's documented `transactions_list`.
    list: (page = 1) => invokeCommand<TransactionsPage>('transactions_list', { page }),
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
    get: (id: string) => invokeCommand<any>('transactions_get', { id }),
    getObservations: (id: string) => invokeCommand<any[]>('fetch_transaction_observations', { transactionId: id }),
    getSourceLog: (id: string) => invokeCommand<string>('fetch_transaction_source_log', { transactionId: id }),
    // G13 fix: the tag names currently on a transaction (relational, not free-text).
    getTags: (id: string) => invokeCommand<string[]>('fetch_transaction_tags', { transactionId: id }),
    addTag: (transactionId: string, tagId: string) =>
      invokeCommand<string>('transactions_add_tag', { transactionId, tagId }),
    removeTag: (transactionId: string, tagId: string) =>
      invokeCommand<string>('transactions_remove_tag', { transactionId, tagId }),
    getEmiGroup: (emiGroupId: string) =>
      invokeCommand<{ installments_paid: number; total_paid_minor: number }>('transactions_get_emi_group', { emiGroupId }),
  },
  tags: {
    // G13 fix: the full reusable-tag catalog, for autocomplete.
    list: () => invokeCommand<string[]>('tags_list'),
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
    // G20/H10/J8 fix: action vocabulary realigned to Doc 19 §10.3's
    // documented "Allowed actions" ('confirm_match'/'reject_candidate'
    // replace the previous 'merge'/'reject' — 'keep_separate' and
    // 'mark_unresolved' already matched).
    resolve: (
      clusterId: string,
      action: 'confirm_match' | 'reject_candidate' | 'keep_separate' | 'mark_unresolved',
      chosenCanonicalId?: string,
    ) =>
      invokeCommand<string>('reconciliation_clusters_resolve', {
        clusterId,
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
    // such command existed at all.
    exportData: (exportPath: string) => invokeCommand<string>('settings_export_data', { exportPath }),
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
