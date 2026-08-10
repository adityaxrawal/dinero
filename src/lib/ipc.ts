/**
 * The complete typed surface between the React frontend and the Rust backend.
 *
 * Every backend call in the application goes through this file, and that is the
 * point: `invoke` is stringly-typed and returns `any`, so calling it directly
 * from components would spread untyped, unlogged access across the codebase.
 * Funnelling it through one module buys three things at once -- a TypeScript
 * signature for each command, automatic timing and logging of every call, and a
 * single place where backend errors are normalised into the AppError contract.
 *
 * The file has two halves. First the payload interfaces, which mirror the Rust
 * structs on the other side; note their snake_case fields, which are serde's
 * output and are deliberately left unconverted so the shapes stay directly
 * comparable to their Rust definitions. Then the `API` object, grouped by
 * domain (dashboard, transactions, statements, reconciliation, and so on) to
 * give call sites a discoverable namespace.
 *
 * Keeping these interfaces in step with the Rust structs is manual. A field
 * renamed in Rust will compile fine here and arrive as undefined at runtime.
 */
/* eslint-disable @typescript-eslint/no-explicit-any */
import { invoke } from '@tauri-apps/api/core';
import { readFile } from '@tauri-apps/plugin-fs';
import type { AppError } from '@/types/ipc';
import { logger } from './logger';

/**
 * The single wrapper every command below is routed through.
 *
 * Times the round trip, logs both outcomes, and guarantees that whatever the
 * caller catches is an AppError -- never a bare string or an unknown object.
 * That guarantee is what lets the error-mapping layer branch on `code` alone.
 */
async function invokeCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const start = performance.now();
  try {
    const res = await invoke<T>(command, args);
    const duration = performance.now() - start;
    // The logging command itself is excluded from logging, since recording a
    // log call would emit another log call and recurse indefinitely.
    if (command !== 'log_frontend_event') {
      logger.apiCall(command, args, duration, true);
    }
    return res;
  } catch (error) {
    const duration = performance.now() - start;
    // Rust errors that crossed the boundary as structured payloads are already
    // the right shape and pass through; anything else (a panic message, a
    // plugin failure, a thrown string) is wrapped so downstream code can rely
    // on `code` and `message` existing. The original value is preserved under
    // `details` rather than discarded.
    const structuredErr =
      typeof error === 'object' && error !== null && 'code' in error && 'message' in error
        ? (error as AppError)
        : ({
            code: 'UNKNOWN_ERROR',
            message: typeof error === 'string' ? error : 'An unknown error occurred.',
            details: { original: error },
          } as AppError);

    if (command !== 'log_frontend_event') {
      logger.apiCall(command, args, duration, false, structuredErr);
    }
    throw structuredErr;
  }
}

// ---------------------------------------------------------------------------
// Payload types
//
// TypeScript mirrors of the Rust structs these commands return. Fields stay in
// serde's snake_case so each shape lines up with its Rust definition, and the
// pervasive `| null` reflects a real property of this domain: extraction from
// bank emails and PDFs is best-effort, so almost any field can be absent on a
// transaction the parser only partially recovered.
// ---------------------------------------------------------------------------

interface DashboardSummary {
  month_to_date_spend: number;
  limit: number;
  utilization_pct: number;
  recent_transactions_count: number;
  upcoming_bills_count: number;
  income: number;
}

interface UpcomingBill {
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

interface PendingReviewMetric {
  count: number;
  amount_minor: number;
}

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
  channel: string | null;
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
  channel: string | null;
  is_deleted: boolean;
  created_at: string | null;
  updated_at: string | null;
}

interface MatchDecision {
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

interface TransactionDetailResponse {
  transaction: CanonicalTransaction;
  observations: TransactionObservation[];
  match_decisions: MatchDecision[];
}

interface EmiInstallmentDetail {
  transaction_id: string;
  amount_minor: number;
  event_time: string | null;
}

interface EmiGroupSummary {
  installments_paid: number;
  total_paid_minor: number;
  total_installments: number | null;
  installments: EmiInstallmentDetail[];
}

export interface UnprocessedStatementEntry {
  statement_id: string;
  filename: string;
  display_name: string | null;
  failure_type: string | null;
  failure_reason: string | null;
  sender?: string;
  to?: string;
  subject?: string;
  date?: string;
  snippet?: string;
  html?: string;
}

interface AwaitingReviewEntry {
  draft_id: string;
  issuer_name: string | null;
  masked_identifier: string | null;
  origin: string;
  created_at: string | null;
}

export interface StatementReparseProgress {
  processed: number;
  total: number;
  current?: string;
  parsed?: number;
  still_locked?: number;
  bytes_expired?: number;
  failed?: number;
  done: boolean;
}

interface UnprocessedStatementGroups {
  awaiting_password: UnprocessedStatementEntry[];
  pending_retry: UnprocessedStatementEntry[];
  failed: UnprocessedStatementEntry[];
  awaiting_review: AwaitingReviewEntry[];
}

export interface DraftRow {
  transaction_date: string;
  merchant_raw: string;
  amount_minor: number;
  currency: string;
  direction: 'debit' | 'credit';
  reference_id: string | null;
  row_index: number;
  llm_extracted: boolean;
}

export interface StatementDraft {
  id: string;
  origin: 'password_unlock' | 'manual_upload' | 'email_scan';
  issuer_name: string | null;
  masked_identifier: string | null;
  instrument_type: string | null;
  billing_period_start: string | null;
  billing_period_end: string | null;
  due_date: string | null;
  statement_date: string | null;
  current_balance: number | null;
  minimum_due: number | null;
  rows: DraftRow[];
  status: string;
}

export interface DraftMetadataInput {
  issuerName: string;
  maskedIdentifier: string;
  instrumentType: string;
  billingPeriodStart: string | null;
  billingPeriodEnd: string | null;
  dueDate: string | null;
  statementDate: string | null;
  currentBalance: number | null;
  minimumDue: number | null;
}

export interface ProcessingProgressPayload {
  draft_id: string | null;
  instrument_id: string;
  stage:
    | 'parsing'
    | 'metadata'
    | 'instrument_check'
    | 'duplicate_check'
    | 'extracting_rows'
    | 'staged'
    | 'failed';
  percent: number;
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
  direction: string | null;
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
  issuer_name: string | null;
  masked_identifier: string | null;
  instrument_type: string | null;
  pdf_available: boolean;
}

export type ClusterMemberRole = 'incoming' | 'candidate_a' | 'candidate_b' | 'candidate_other';

export interface ClusterMember {
  id: string;
  member_role: ClusterMemberRole;
  observation_id: string | null;
  canonical_transaction_id: string | null;
  source_pipeline: string | null;
  merchant: string;
  amount: number;
  direction: string | null;
  date: string;
  instrument_issuer_name: string | null;
  instrument_masked_identifier: string | null;
  reference_id: string | null;
  match_score: number | null;
  source_raw_payload_json: string | null;
}

export interface ClusterRecord {
  id: string;
  reason: string;
  members_count: number;
  members: ClusterMember[];
  created_at: string | null;
  explanation: string;
}

export interface UnassignedTransactionRecord {
  id: string;
  observation_id: string;
  reason: string;
  status: string;
  created_at: string | null;
  merchant_raw: string | null;
  amount_minor: number | null;
  currency: string | null;
  direction: string | null;
  event_time: string | null;
  source_message_id: string | null;
  body_snippet: string | null;
  raw_payload_json: string | null;
  extraction_method?: string | null;
  confidence_score?: number | null;
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
  nickname?: string;
  network?: string;
  account_type?: string;
  upi_vpa?: string;
  rewards_summary?: string;
  statement_due_date?: string;
  minimum_due?: number;
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

export interface ReleaseReadinessLocalMetrics {
  unresolved_clusters: number;
  llm_fallback_rate: number;
  db_size_bytes: number;
  statement_parse_failure_rate: number;
}

export interface ReleaseReadinessSnapshot {
  id: string;
  captured_at: string;
  metrics: ReleaseReadinessLocalMetrics;
  go_no_go: boolean;
}

interface BackendStatus {
  status: 'healthy' | 'corrupted' | 'locked';
}

interface HealthReport {
  backend_ready: boolean;
  db_integrity_ok: boolean;
  checkpoint_age_seconds: number | null;
  gmail_polling_status: 'not_connected' | 'active' | 'degraded' | 'quota_exhausted' | 'unknown';
  license_status: string;
}

export interface ConnectedAccountInfo {
  email: string;
  account_id: string;
  account_status: string;
}

export interface CategoryBudget {
  name: string;
  budget: number;
}

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

export interface LlmHardwareInfo {
  ram_gb: number;
  cpu_cores: number;
  recommended_slots: number;
  recommended_model_id: string | null;
}

export interface MerchantCleanupSample {
  transaction_id: string;
  merchant: string;
  bank_name: string;
  confidence: number;
  has_evidence: boolean;
  amount: number | null;
  currency: string | null;
  direction: string | null;
  event_time: string | null;
}

export interface MerchantCleanupBankBucket {
  bank_name: string;
  count: number;
  no_evidence: number;
}

export interface MerchantCleanupPreview {
  candidate_count: number;
  no_evidence_count: number;
  by_bank: MerchantCleanupBankBucket[];
  samples: MerchantCleanupSample[];
  llm_eligible: boolean;
  total_ram_gb: number;
  running: boolean;
}

export interface MerchantCleanupProgress {
  run_id: string;
  processed: number;
  total: number;
  applied: number;
  skipped: number;
  current_merchant: string | null;
  bank_name: string | null;
  resolved_merchant: string | null;
  resolved_category: string | null;
  status: 'running' | 'completed' | 'cancelled' | 'failed';
}

export interface MerchantCleanupChange {
  correction_id: string;
  transaction_id: string;
  bank_name: string;
  previous_merchant: string | null;
  new_merchant: string | null;
  category: string | null;
  confidence: number;
  reverted: boolean;
}

export interface MerchantCleanupRun {
  run_id: string;
  started_at: string | null;
  applied: number;
  reverted: number;
  banks: string[];
  changes: MerchantCleanupChange[];
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

/**
 * The command surface, namespaced by domain.
 *
 * Each leaf is a thin typed binding onto a Rust command name. Where the backend
 * returns a wrapper object around a single collection, the binding unwraps it
 * with `.then()` so callers receive the array directly rather than having to
 * reach through a redundant envelope at every call site.
 */
export const API = {
  // Aggregate figures for the dashboard's summary tiles.
  dashboard: {
    getSummary: () => invokeCommand<DashboardSummary>('dashboard_summary'),
    getUpcomingBills: () =>
      invokeCommand<{ bills: UpcomingBill[] }>('dashboard_upcoming_bills').then((r) => r.bills),
    getCategories: (month: string) =>
      invokeCommand<{ categories: CategorySpend[] }>('dashboard_categories', { month }).then(
        (r) => r.categories
      ),
  },

  // Derived time series and review metrics that back the charts.
  analytics: {
    getSpendTrend: (granularity: SpendTrendGranularity) =>
      invokeCommand<SpendTrendPoint[]>('analytics_spend_trend', { granularity }),
    getPendingReviewCount: () =>
      invokeCommand<PendingReviewMetric>('analytics_pending_review_count'),
  },
  // The core ledger: listing, search, manual entry, edits, and the per-
  // transaction detail panels (observations, tags, source provenance).
  transactions: {
    list: (page = 1, filters?: TransactionListFilters) =>
      invokeCommand<TransactionsPage>('transactions_list', { page, filters }),
    search: (query: string, filters?: TransactionListFilters) =>
      invokeCommand<TransactionRecord[]>('transactions_search', { query, filters }),
    create: (input: {
      amountMinor: number;
      currency: string;
      direction: 'credit' | 'debit';
      eventTime: string;
      merchantName: string;
      instrumentId: string;
      referenceId?: string;
    }) =>
      invokeCommand<string>('transactions_create', {
        payload: {
          amount_minor: input.amountMinor,
          currency: input.currency,
          direction: input.direction,
          event_time: input.eventTime,
          merchant_name: input.merchantName,
          instrument_id: input.instrumentId,
          reference_id: input.referenceId ?? null,
        },
      }),
    delete: (transactionId: string) =>
      invokeCommand<string>('transactions_delete', { transactionId }),
    update: (
      transactionId: string,
      updates: {
        merchantDisplayName?: string | undefined;
        categoryId?: string | undefined;
        notes?: string | undefined;
        location?: string | undefined;
        tags?: string[] | undefined;
        amountMinor?: number | undefined;
        direction?: string | undefined;
        eventTime?: string | undefined;
        instrumentId?: string | undefined;
      }
    ) =>
      invokeCommand<string>('transactions_update', {
        payload: {
          transaction_id: transactionId,
          merchant_display_name: updates.merchantDisplayName,
          category_id: updates.categoryId,
          notes: updates.notes,
          location: updates.location,
          tags: updates.tags,
          amount_minor: updates.amountMinor,
          direction: updates.direction,
          event_time: updates.eventTime,
          instrument_id: updates.instrumentId,
        },
      }),
    reportWrongBank: (transactionId: string, domain: string, bankName: string) =>
      invokeCommand<void>('feedback_report_wrong_bank', { transactionId, domain, bankName }),
    get: (id: string) => invokeCommand<TransactionDetailResponse>('transactions_get', { id }),
    getObservations: (id: string) =>
      invokeCommand<TransactionObservation[]>('fetch_transaction_observations', {
        transactionId: id,
      }),
    getSourceLog: (id: string) =>
      invokeCommand<string>('fetch_transaction_source_log', { transactionId: id }),
    getTags: (id: string) =>
      invokeCommand<string[]>('fetch_transaction_tags', { transactionId: id }),
    addTag: (transactionId: string, tagId: string) =>
      invokeCommand<string>('transactions_add_tag', { transactionId, tagId }),
    removeTag: (transactionId: string, tagId: string) =>
      invokeCommand<string>('transactions_remove_tag', { transactionId, tagId }),
    getEmiGroup: (emiGroupId: string) =>
      invokeCommand<EmiGroupSummary>('transactions_get_emi_group', { emiGroupId }),
  },
  // Free-form user labels applied across transactions.
  tags: {
    list: () => invokeCommand<TagRecord[]>('tags_list'),
    create: (name: string) =>
      invokeCommand<{ id: string; status: string }>('tags_create', { payload: { name } }),
  },
  // The fixed spending taxonomy transactions are classified into.
  categories: {
    list: () => invokeCommand<CategoryRecord[]>('categories_list'),
  },
  // PDF statement ingestion, from upload through password unlock, parsing and
  // draft review, to the retry paths for statements that failed.
  statements: {
    upload: async (filePaths: string[]) => {
      const files = await Promise.all(
        filePaths.map(async (path) => {
          const bytes = await readFile(path);
          const filename = path.split(/[/\\]/).pop() || path;
          return { file_bytes: Array.from(bytes), filename };
        })
      );
      const response = await invokeCommand<{
        results: Array<{
          status: string;
          statement_id?: string;
          filename?: string;
          error?: string;
        }>;
      }>('statements_upload', { files });
      return response.results;
    },
    submitPassword: (statementId: string, instrumentId: string, password: string) =>
      invokeCommand<{
        status: string;
        statement_id?: string;
        draft_id?: string;
        attempts_remaining?: number;
      }>('statements_submit_password', { statementId, instrumentId, password }),
    confirmInstrument: (
      statementId: string,
      issuerName: string,
      maskedIdentifier: string,
      instrumentType: string
    ) =>
      invokeCommand<{ status: string; statement_id?: string; draft_id?: string }>(
        'statements_confirm_instrument',
        {
          statementId,
          issuerName,
          maskedIdentifier,
          instrumentType,
        }
      ),
    listUnprocessed: () => invokeCommand<UnprocessedStatementGroups>('statements_list_unprocessed'),
    retryUnprocessed: (statementId: string) =>
      invokeCommand<{ status: string; statement_id: string }>('statements_retry_unprocessed', {
        statementId,
      }),
    reparseAll: () => invokeCommand<StatementReparseProgress>('statements_reparse_all'),
    discard: (statementId: string) =>
      invokeCommand<{ status: string }>('statements_discard', { statementId }),
    listHistory: (page = 1) =>
      invokeCommand<{ records: StatementRecord[]; total: number }>('statements_list', {
        page,
      }).then((res) => res.records),
    getEntries: (statementId: string) =>
      invokeCommand<any[]>('statements_get_entries', { statementId }),
    getPdf: (statementId: string) => invokeCommand<string>('statements_get_pdf', { statementId }),
    deletePdf: (statementId: string) =>
      invokeCommand<void>('statements_delete_pdf', { statementId }),
    getDraft: (draftId: string) =>
      invokeCommand<StatementDraft>('statements_get_draft', { draftId }),
    getDraftPdf: (draftId: string) =>
      invokeCommand<string>('statements_get_draft_pdf', { draftId }),
    commitDraft: (draftId: string, metadata: DraftMetadataInput, rows: DraftRow[]) =>
      invokeCommand<{ status: string; statement_id: string }>('statements_commit_draft', {
        draftId,
        editedMetadata: {
          issuer_name: metadata.issuerName,
          masked_identifier: metadata.maskedIdentifier,
          instrument_type: metadata.instrumentType,
          billing_period_start: metadata.billingPeriodStart,
          billing_period_end: metadata.billingPeriodEnd,
          due_date: metadata.dueDate,
          statement_date: metadata.statementDate,
          current_balance: metadata.currentBalance,
          minimum_due: metadata.minimumDue,
        },
        editedRows: rows,
      }),
    discardDraft: (draftId: string) =>
      invokeCommand<{ status: string }>('statements_discard_draft', { draftId }),
  },
  // Duplicate resolution: clusters of transactions that may be the same real
  // payment seen through different sources, and the merge/split decisions on them.
  reconciliation: {
    listUnresolved: () => invokeCommand<ClusterRecord[]>('reconciliation_clusters_list'),
    getCluster: (clusterId: string) =>
      invokeCommand<ClusterRecord>('reconciliation_clusters_get', { clusterId }),
    listUnassigned: () =>
      invokeCommand<UnassignedTransactionRecord[]>('reconciliation_get_unassigned_transactions'),
    dismissUnassigned: (id: string) =>
      invokeCommand<void>('reconciliation_dismiss_unassigned_transaction', { id }),
    resolveUnassignedManually: (
      id: string,
      payload: {
        amountMinor: number;
        currency: string;
        direction: 'credit' | 'debit';
        eventTime: string;
        merchantName: string;
        instrumentId: string;
        referenceId?: string | undefined;
      }
    ) =>
      invokeCommand<string>('reconciliation_resolve_unassigned_transaction_manually', {
        id,
        payload: {
          amount_minor: payload.amountMinor,
          currency: payload.currency,
          direction: payload.direction,
          event_time: payload.eventTime,
          merchant_name: payload.merchantName,
          instrument_id: payload.instrumentId,
          reference_id: payload.referenceId ?? null,
        },
      }),
    resolve: (
      clusterId: string,
      observationId: string,
      action: 'confirm_match' | 'reject_candidate' | 'keep_separate' | 'mark_unresolved',
      chosenCanonicalId?: string
    ) =>
      invokeCommand<string>('reconciliation_clusters_resolve', {
        clusterId,
        observationId,
        action,
        chosenCanonicalId,
      }),
    unmergeCluster: (clusterId: string) =>
      invokeCommand<string>('reconciliation_clusters_unmerge', { clusterId }),
  },
  // Payment instruments (cards, accounts) that transactions are attributed to.
  instruments: {
    list: () => invokeCommand<InstrumentRecord[]>('instruments_list'),
    create: (
      instrumentType: string,
      issuerName: string,
      maskedIdentifier: string,
      fullIdentifier?: string,
      billingCycleDay?: number,
      bankIfsc?: string
    ) =>
      invokeCommand<InstrumentRecord>('instruments_create', {
        payload: {
          instrument_type: instrumentType,
          issuer_name: issuerName,
          masked_identifier: maskedIdentifier,
          full_identifier: fullIdentifier,
          billing_cycle_day: billingCycleDay,
          bank_ifsc: bankIfsc,
        },
      }),
    update: (
      id: string,
      fullIdentifier?: string,
      billingCycleDay?: number,
      bankIfsc?: string,
      extra?: {
        nickname?: string;
        credit_limit?: number;
        account_type?: string;
        network?: string;
        status?: string;
        upi_vpa?: string;
        rewards_summary?: string;
        instrument_type?: string;
        issuer_name?: string;
        masked_identifier?: string;
        current_balance?: number;
        statement_due_date?: string;
        minimum_due?: number;
      }
    ) =>
      invokeCommand<string>('instruments_update', {
        payload: {
          id,
          full_identifier: fullIdentifier,
          billing_cycle_day: billingCycleDay,
          bank_ifsc: bankIfsc,
          ...extra,
        },
      }),
    get: (id: string) => invokeCommand<InstrumentRecord>('instruments_get', { id }),
    delete: (id: string) => invokeCommand<string>('instruments_archive', { id }),
  },
  // Budget ceilings and the thresholds that trigger alerts.
  spendingLimits: {
    get: () => invokeCommand<SpendingLimits>('fetch_spending_limits'),
    update: (limits: SpendingLimits) => invokeCommand<string>('update_spending_limits', { limits }),
  },
  // First-run setup: preferences captured before the main app is usable.
  onboarding: {
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
  // Backend liveness and health, polled by the shell's engine indicator.
  status: {
    check: () => invokeCommand<BackendStatus>('check_backend_status'),
    getHealthReport: () => invokeCommand<HealthReport>('get_health_report'),
  },
  // Whole-database operations: backup restore, export, and deletion.
  db: {
    restoreBackup: () => invokeCommand<string>('db_restore_backup'),
    exportData: (exportPath: string, password?: string) =>
      invokeCommand<string>('settings_export_data', { exportPath, password: password ?? null }),
  },
  // Development-only helpers. Destructive; not reachable in normal use.
  dev: {
    resetDatabase: () => invokeCommand<string>('settings_delete_account'),
    getMetrics: () => invokeCommand<DebugMetrics>('get_debug_metrics'),
    checkSystemRam: () => invokeCommand<number>('check_system_ram'),
  },
  // Diagnostics the user can send to support: log export and feedback.
  support: {
    exportLogs: () => invokeCommand<{ success: boolean; file_path: string }>('export_logs'),
    submitFeedback: (text: string, includeLogs: boolean) =>
      invokeCommand<string>('submit_user_feedback', { text, includeLogs }),
    logRendererError: (message: string, stack: string | undefined, source: string) =>
      invokeCommand<void>('log_renderer_error', { message, stack, source }),
  },
  // Google OAuth and connected-account management.
  auth: {
    startGoogle: () => invokeCommand<string>('auth_google_start'),
    isGmailConnected: () => invokeCommand<boolean>('is_gmail_connected'),
    listConnectedAccounts: () =>
      invokeCommand<ConnectedAccountInfo[]>('settings_get_connected_accounts'),
    disconnectGmail: (accountId: string) =>
      invokeCommand<void>('auth_google_disconnect', { accountId }),
    getRecoveryPhrase: () => invokeCommand<string>('auth_get_recovery_phrase'),
  },
  // Historical Gmail scans: start, cancel, and status.
  ingestion: {
    startHistoricalScan: (accountId: string, startDate: string, endDate: string) =>
      invokeCommand<string>('scans_historical', { accountId, startDate, endDate }),
    cancelScan: (accountId: string) => invokeCommand<string>('scans_cancel', { accountId }),
    getScanStatus: (accountId: string) =>
      invokeCommand<ScanStatusResponse>('scans_status', { accountId }),
  },
  // Local LLM lifecycle: model catalogue, download, activation, concurrency.
  llm: {
    getAvailableModels: () => invokeCommand<LlmModelInfo[]>('llm_get_available_models'),
    downloadModel: (modelId: string) => invokeCommand<void>('llm_download_model', { modelId }),
    deleteModel: (modelId: string) => invokeCommand<string>('llm_delete_model', { modelId }),
    cancelDownload: (modelId: string) => invokeCommand<void>('llm_cancel_download', { modelId }),
    getDownloadedModels: () => invokeCommand<string[]>('llm_get_downloaded_models'),
    getActiveModel: () => invokeCommand<string>('llm_get_active_model'),
    setActiveModel: (modelId: string) => invokeCommand<void>('llm_set_active_model', { modelId }),
    getHardwareInfo: () => invokeCommand<LlmHardwareInfo>('llm_get_hardware_info'),
    setParallelSlots: (slots: number) =>
      invokeCommand<number>('llm_set_parallel_slots', { slots }),
  },
  // AI merchant-name normalisation: preview, run, and review its changes.
  merchantCleanup: {
    preview: () => invokeCommand<MerchantCleanupPreview>('merchant_cleanup_preview'),
    start: () => invokeCommand<string>('merchant_cleanup_start'),
    cancel: () => invokeCommand<void>('merchant_cleanup_cancel'),
    revert: (runId: string) => invokeCommand<number>('merchant_cleanup_revert', { runId }),
    runs: (limit = 20) => invokeCommand<MerchantCleanupRun[]>('merchant_cleanup_runs', { limit }),
    revertCorrection: (correctionId: string) =>
      invokeCommand<void>('merchant_cleanup_revert_correction', { correctionId }),
  },
  // Debug inspection surfaces. Return `any` deliberately -- these feed a raw
  // JSON viewer, so imposing a type would add no safety and constant churn.
  debug: {
    fetchParseErrors: () => invokeCommand<any[]>('debug_fetch_parse_errors'),
    fetchUnprocessedStatements: () => invokeCommand<any[]>('debug_fetch_unprocessed_statements'),
    fetchAuditLog: (resourceTypeFilter?: string, limit: number = 50, offset: number = 0) =>
      invokeCommand<any[]>('debug_fetch_audit_log', { resourceTypeFilter, limit, offset }),
    fetchReconciliationClusters: () => invokeCommand<any[]>('debug_fetch_reconciliation_clusters'),
    getPipelineState: () => invokeCommand<any>('debug_get_pipeline_state'),
    setGmailPollPaused: (paused: boolean) =>
      invokeCommand<void>('debug_set_gmail_poll_paused', { paused }),
    setScanQueuePaused: (paused: boolean) =>
      invokeCommand<void>('debug_set_scan_queue_paused', { paused }),
    captureReleaseReadinessSnapshot: () =>
      invokeCommand<ReleaseReadinessSnapshot>('release_readiness_capture_snapshot'),
    listReleaseReadinessSnapshots: () =>
      invokeCommand<ReleaseReadinessSnapshot[]>('release_readiness_list_snapshots'),
  },
  // The outbound-request audit log backing the privacy disclosure screen.
  network: {
    getActivityList: () =>
      invokeCommand<{ entries: any[] }>('settings_get_network_activity').then((r) => r.entries),
  },
  // Rules the app inferred from user corrections, listable and revertible.
  learnedRules: {
    list: () => invokeCommand<LearnedRule[]>('settings_learned_rules_list'),
    revert: (ruleId: string) => invokeCommand<void>('settings_learned_rules_revert', { ruleId }),
  },
  senderOverrides: {
    list: () => invokeCommand<SenderBankOverride[]>('settings_sender_overrides_list'),
    revert: (id: string) => invokeCommand<void>('settings_sender_overrides_revert', { id }),
    knownBankNames: () => invokeCommand<string[]>('settings_known_bank_names'),
  },
  pdfPasswords: {
    list: () => invokeCommand<PdfPasswordSummary[]>('settings_pdf_passwords_list'),
    delete: (id: string) => invokeCommand<void>('settings_pdf_passwords_delete', { id }),
  },
  privacy: {
    getConsentHistory: (limit = 50, offset = 0) =>
      invokeCommand<ConsentEventRecord[]>('auth_get_consent_history', { limit, offset }),
    recordConsentEvent: (consentType: string, detail: string) =>
      invokeCommand<void>('record_consent_event', { consentType, detail }),
  },
  licensing: {
    getStatus: () => invokeCommand<LicenseStatusResponse>('license_get_status'),
    activate: (
      email: string,
      razorpayPaymentId: string,
      razorpaySignature: string,
      billingInterval: string
    ) =>
      invokeCommand<LicenseActivateResponse>('license_activate', {
        email,
        razorpayPaymentId,
        razorpaySignature,
        billingInterval,
      }),
    deactivate: () => invokeCommand<LicenseDeactivateResponse>('license_deactivate'),
    refresh: () => invokeCommand<LicenseRefreshResponse>('license_refresh'),
    startCheckout: (email: string, planId: string) =>
      invokeCommand<{ razorpay_payment_id: string; razorpay_signature: string }>(
        'billing_start_checkout',
        { email, planId }
      ),
  },
  updater: {
    confirmInstall: () => invokeCommand<void>('updater_confirm_install'),
  },
  systemWarnings: {
    getActive: () => invokeCommand<SystemWarningPayload[]>('get_active_system_warnings'),
    dismiss: (warningType: string) =>
      invokeCommand<void>('settings_dismiss_system_warning', { warningType }),
  },
  backgroundTasks: {
    getActive: () => invokeCommand<BackgroundTaskProgressPayload[]>('get_active_background_tasks'),
  },
  menuBarExtra: {
    getEnabled: () => invokeCommand<boolean>('settings_get_menu_bar_extra_enabled'),
    setEnabled: (enabled: boolean) =>
      invokeCommand<void>('settings_set_menu_bar_extra_enabled', { enabled }),
  },
  lifecycle: {
    getLaunchAtLogin: () => invokeCommand<boolean>('settings_get_launch_at_login'),
    setLaunchAtLogin: (enabled: boolean) =>
      invokeCommand<void>('settings_set_launch_at_login', { enabled }),
    getBackgroundSyncEnabled: () => invokeCommand<boolean>('settings_get_background_sync_enabled'),
    setBackgroundSyncEnabled: (enabled: boolean) =>
      invokeCommand<void>('settings_set_background_sync_enabled', { enabled }),
    getLowBatteryPollThresholdPercent: () =>
      invokeCommand<number>('settings_get_low_battery_poll_threshold_percent'),
    setLowBatteryPollThresholdPercent: (thresholdPercent: number) =>
      invokeCommand<void>('settings_set_low_battery_poll_threshold_percent', {
        thresholdPercent,
      }),
  },
};

export interface SystemWarningPayload {
  warning_type: string;
  message: string;
  severity: 'critical' | 'degraded' | 'info';
  action_hint: string | null;
}

export interface BackgroundTaskProgressPayload {
  task_id: string;
  task_type: string;
  label: string;
  current: number;
  total: number;
  eta_seconds: number | null;
  status: 'running' | 'completed' | 'failed';
  progress_pct: number;
  status_message: string;
}

export interface LicenseStatusResponse {
  state: string;
  is_active: boolean;
  license_key_masked: string | null;
  plan_id: string | null;
  billing_interval: string | null;
  expiry_date: string | null;
  days_remaining: number | null;
}

interface LicenseActivateResponse {
  status: string;
  state: string;
  device_bound: boolean;
  plan_id: string | null;
  billing_interval: string | null;
  expires_at: string | null;
}

interface LicenseDeactivateResponse {
  status: string;
  state: string;
}

interface LicenseRefreshResponse {
  status: string;
  state: string;
}

export type LearnedRuleStatus = 'pending' | 'active' | 'trusted' | 'inactive' | 'flagged';

export interface LearnedRule {
  id: string;
  bank_name: string;
  field_name: string;
  source_type: 'email' | 'statement_pdf';
  template_hash: string;
  rule_payload_json: { regex?: string; capture_group?: number; override_value?: string };
  status: LearnedRuleStatus;
  success_count: number;
  failure_count: number;
  confidence: number;
  authored_by: 'deterministic' | 'llm';
  learned_from: 'user_edit' | 'drift_llm' | 'batch_cleanup';
  created_at: string | null;
  updated_at: string | null;
}

export interface SenderBankOverride {
  id: string;
  domain: string;
  bank_name: string;
  display_name: string | null;
  status: 'active' | 'inactive';
  created_at: string | null;
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

export interface ScanStatusResponse {
  status: string;
  processed: number;
  total: number;
  transactions_found: number;
  statements_found: number;
  mandate_events_found: number;
  errors: number;
  pending_enrichment: number;
}

export interface ScanProgressPayload {
  account_id: string;
  processed: number;
  total: number;
  transactions_found: number;
  statements_found: number;
  mandate_events_found: number;
  non_financial: number;
  errors: number;
  pending_enrichment: number;
  error_message?: string | null;
}
