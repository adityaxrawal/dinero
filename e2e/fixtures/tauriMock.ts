import { test as base } from '@playwright/test';

// Define the mock handlers inside a string to be evaluated in the browser context
const tauriMockInitScript = `
  window.__TAURI_INTERNALS__ = window.__TAURI_INTERNALS__ || {};
  
  // Provide dummy transformCallback to prevent TypeError from @tauri-apps/api/event
  window.__TAURI_INTERNALS__.transformCallback = function(callback, once) {
    const identifier = Math.floor(Math.random() * 1000000);
    window['_' + identifier] = (result) => {
      if (once) {
        delete window['_' + identifier];
      }
      callback?.(result);
    };
    return identifier;
  };
  
  // State for mocks
  window.__MOCK_STATE__ = {
    tx_update_failure: false,
    gmail_failure: false,
    password_failure: false,
    resolve_failure: false,
    instrument_conflict: false,
    instrument_delete_tied: false,
    unprocessed_statements: { awaiting_password: [], pending_retry: [], failed: [] },
  };

  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    if (cmd === 'dashboard_summary') {
      // TASK-FE-008 fix: this mock still had the pre-TASK-API-006 shape
      // (total_spend/upcoming_bills) -- the real DashboardSummary has been
      // month_to_date_spend/limit/utilization_pct/recent_transactions_count/
      // upcoming_bills_count/income since that task, so Dashboard.spec.ts
      // never actually completed its loading state against this fixture.
      return {
        month_to_date_spend: 2199.0,
        limit: 60000.0,
        utilization_pct: 3.665,
        recent_transactions_count: 3,
        upcoming_bills_count: 2,
        income: 110000.0,
      };
    }
    if (cmd === 'dashboard_upcoming_bills') {
      return {
        bills: [
          { id: 'bill_1', description: 'HDFC Bank Credit Card', amount: 4500.0, currency: 'INR', due_date: new Date(Date.now() + 2 * 86400000).toISOString().slice(0, 10) },
          { id: 'bill_2', description: 'ICICI Bank Credit Card', amount: 1200.0, currency: 'INR', due_date: new Date(Date.now() + 10 * 86400000).toISOString().slice(0, 10) },
        ],
      };
    }
    if (cmd === 'dashboard_categories') {
      return {
        categories: [
          { category_id: 'cat_food', name: 'FOOD', total_spend: 899.0, monthly_budget: 8000, utilization_pct: 11.2, currency: 'INR' },
          { category_id: 'cat_transport', name: 'TRANSPORT', total_spend: 250.0, monthly_budget: 4000, utilization_pct: 6.25, currency: 'INR' },
          { category_id: 'cat_shopping', name: 'SHOPPING', total_spend: 1050.0, monthly_budget: 15000, utilization_pct: 7.0, currency: 'INR' },
          { category_id: 'cat_empty', name: 'ENTERTAINMENT', total_spend: 0, monthly_budget: 5000, utilization_pct: 0, currency: 'INR' },
        ],
      };
    }
    if (cmd === 'analytics_spend_trend') {
      return [
        { period: '2026-05', total_spend: 1800.0 },
        { period: '2026-06', total_spend: 2199.0 },
      ];
    }
    if (cmd === 'analytics_pending_review_count') {
      return { count: 0, amount_minor: 0 };
    }
    if (cmd === 'transactions_list') {
      // G9 fix (pre-existing mock/contract drift, found during the G20/H10/J8
      // rename): the real backend returns { records, total }, not a bare
      // array — this mock was never updated when that pagination shape
      // shipped, so any test that actually rendered Transactions crashed on
      // records.map() over undefined.
      const records = [
        { id: 'tx_1', date: '2026-06-10T14:32:00Z', merchant: 'Amazon Pay India', amount: -1499.0, category: 'SHOPPING', status: 'POSTED', tags: ['online'], source_mix: 'merged' },
        { id: 'tx_2', date: '2026-06-09T20:15:00Z', merchant: 'Swiggy', amount: -450.0, category: 'FOOD', status: 'POSTED', tags: ['dinner'], source_mix: 'email_only' },
        { id: 'tx_3', date: '2026-06-08T09:00:00Z', merchant: 'Uber', amount: -250.0, category: 'TRANSPORT', status: 'POSTED', tags: [], source_mix: 'statement_only' },
      ];
      return { records, total: records.length };
    }
    if (cmd === 'transactions_search') {
      const query = (args?.query || '').toString().toLowerCase();
      const allTxns = [
        { id: 'tx_1', date: '2026-06-10T14:32:00Z', merchant: 'Amazon Pay India', amount: -1499.0, category: 'SHOPPING', status: 'POSTED', tags: ['online'] },
        { id: 'tx_2', date: '2026-06-09T20:15:00Z', merchant: 'Swiggy', amount: -450.0, category: 'FOOD', status: 'POSTED', tags: ['dinner'] },
      ];
      if (!query) return allTxns;
      return allTxns.filter((t) => t.merchant.toLowerCase().includes(query));
    }
    if (cmd === 'fetch_transaction_tags') return ['online'];
    if (cmd === 'transactions_get') {
      // TASK-FE-010 fix: no mock existed at all -- transactions_get had no
      // real frontend call site until this task's detail page.
      return {
        transaction: {
          id: args?.id || 'tx_1', unique_event_id: null, instrument_id: 'inst_1', instrument_type: 'credit_card',
          direction: 'debit', amount: -1499.0, amount_minor: -149900, currency: 'INR',
          authorization_time: '2026-06-10T14:32:00', best_event_time: '2026-06-10T14:32:00',
          event_time_confidence: 'high', best_posting_date: null, posting_date_confidence: null,
          merchant_display_name: 'Amazon Pay India', merchant_normalized_name: 'amazon pay india',
          merchant_entity_id: null, reference_id: null, location: null, original_amount_minor: null,
          original_currency: null, exchange_rate: null, balance_after_transaction: null, status: 'POSTED',
          match_confidence: 'high', source_mix: 'merged', alert_fired: false, parent_transaction_id: null,
          transaction_subtype: null, emi_group_id: null, category_id: 'SHOPPING', is_deleted: false,
          created_at: '2026-06-10T14:32:00', updated_at: '2026-06-10T14:32:00', notes: null,
        },
        observations: [
          {
            id: 'obs_1', canonical_transaction_id: args?.id || 'tx_1', source_pipeline: 'gmail_transaction',
            source_record_id: null, source_message_id: 'msg_1', source_thread_id: null, statement_id: null,
            statement_entry_id: null, instrument_id: 'inst_1', direction: 'debit', amount: -1499.0,
            amount_minor: -149900, currency: 'INR', event_time: '2026-06-10T14:32:00', event_time_confidence: 'high',
            posting_date: null, merchant_raw: 'AMAZON PAY INDIA', merchant_normalized: 'amazon pay india',
            reference_id: null, original_amount_minor: null, original_currency: null, exchange_rate: null,
            balance_after_transaction: null, timezone_at_ingestion: 'Asia/Kolkata', fingerprint: 'fp_1',
            extraction_method: 'layer2_template', confidence_score: 0.95, raw_payload_json: null,
            parser_version: 'v1', emi_total_installments: null, emi_installment_number: null,
            emi_original_amount_minor: null, is_deleted: false, created_at: '2026-06-10T14:32:00', updated_at: '2026-06-10T14:32:00',
          },
        ],
        match_decisions: [],
      };
    }
    if (cmd === 'fetch_transaction_observations') {
      return [
        {
          id: 'obs_1', canonical_transaction_id: args?.transactionId || 'tx_1', source_pipeline: 'gmail_transaction',
          source_record_id: null, source_message_id: 'msg_1', source_thread_id: null, statement_id: null,
          statement_entry_id: null, instrument_id: 'inst_1', direction: 'debit', amount: -1499.0,
          amount_minor: -149900, currency: 'INR', event_time: '2026-06-10T14:32:00', event_time_confidence: 'high',
          posting_date: null, merchant_raw: 'AMAZON PAY INDIA', merchant_normalized: 'amazon pay india',
          reference_id: null, original_amount_minor: null, original_currency: null, exchange_rate: null,
          balance_after_transaction: null, timezone_at_ingestion: 'Asia/Kolkata', fingerprint: 'fp_1',
          extraction_method: 'layer2_template', confidence_score: 0.95, raw_payload_json: null,
          parser_version: 'v1', emi_total_installments: null, emi_installment_number: null,
          emi_original_amount_minor: null, is_deleted: false, created_at: '2026-06-10T14:32:00', updated_at: '2026-06-10T14:32:00',
        },
      ];
    }
    if (cmd === 'transactions_get_emi_group') {
      return {
        installments_paid: 3,
        total_paid_minor: 1500000,
        total_installments: 12,
        installments: [
          { transaction_id: 'tx_1', amount_minor: 500000, event_time: '2026-04-15T10:00:00' },
          { transaction_id: 'tx_2', amount_minor: 500000, event_time: '2026-05-15T10:00:00' },
          { transaction_id: 'tx_3', amount_minor: 500000, event_time: '2026-06-15T10:00:00' },
        ],
      };
    }
    if (cmd === 'fetch_transaction_source_log') {
      return JSON.stringify({ subject: 'Your Amazon Pay transaction', from: 'alerts@amazon.in' });
    }
    // Pre-existing gap (found during the G20/H10/J8 rename): unmocked, so
    // availableTags resolved to the default '{}' fallback instead of an
    // array, crashing the detail panel's tag autocomplete
    // (availableTags.filter().map()) on every row click.
    if (cmd === 'tags_list') return ['online', 'dinner', 'work', 'personal'];
    if (cmd === 'categories_list') {
      // TASK-FE-009 fix: no mock existed at all -- categories_list has had
      // a real frontend call site (the transaction filter bar / quick
      // actions category select) only since this task, and the fixture's
      // unhandled-command fallback ({}) would crash any .map() over it.
      return [
        { id: 'SHOPPING', parent_id: null, name: 'SHOPPING', source_type: 'system', mcc_code: null, monthly_budget_minor: null, is_deleted: false, created_at: null, color: null, icon: null },
        { id: 'FOOD', parent_id: null, name: 'FOOD', source_type: 'system', mcc_code: null, monthly_budget_minor: null, is_deleted: false, created_at: null, color: null, icon: null },
        { id: 'TRANSPORT', parent_id: null, name: 'TRANSPORT', source_type: 'system', mcc_code: null, monthly_budget_minor: null, is_deleted: false, created_at: null, color: null, icon: null },
      ];
    }
    if (cmd === 'transactions_update') {
      if (window.__MOCK_STATE__.tx_update_failure) throw { code: 'UPDATE_FAILED', message: 'Failed to save update' };
      return 'Updated';
    }
    if (cmd === 'statements_list') {
      // TASK-FE-011 fix: this mock returned a bare array, but the real
      // command has returned a paginated { records, total } page since
      // TASK-API-004 -- API.statements.listHistory()'s .then(r => r.records)
      // silently unwrapped to undefined against this stale shape.
      const records = [
        { id: 'stmt_1', date: '2026-06-01T10:00:00Z', file_name: 'HDFC_May_2026.pdf', status: 'PROCESSED', instrument_id: 'inst_1' },
        { id: 'stmt_2', date: '2026-05-01T10:00:00Z', file_name: 'HDFC_Apr_2026.pdf', status: 'PROCESSED', instrument_id: 'inst_1' },
        { id: 'stmt_3', date: '2026-06-05T12:00:00Z', file_name: 'ICICI_May_2026.pdf', status: 'PASSWORD_REQUIRED', instrument_id: 'inst_2' },
      ];
      return { records, total: records.length };
    }
    if (cmd === 'instruments_get') {
      return {
        id: args?.id || 'inst_1', instrument_type: 'credit_card', issuer_name: 'HDFC Bank',
        masked_identifier: '1234', status: 'active', current_balance: -1499.0, credit_limit: 60000.0,
        full_identifier: null, billing_cycle_day: 15, bank_ifsc: null,
      };
    }
    if (cmd === 'reconciliation_clusters_list' || cmd === 'reconciliation_clusters_get') {
      // TASK-FE-013 fix: member shape now mirrors the real Rust
      // ClusterMember (member_role/observation_id/canonical_transaction_id/
      // source_pipeline) instead of a guessed source label -- the old
      // shape had no way to drive reconciliation_clusters_resolve's
      // required observation_id argument at all.
      const clusters = [
        {
          id: 'cluster_1',
          reason: 'Ambiguous match: Same amount on same day',
          members_count: 2,
          members: [
            { id: 'm1', member_role: 'incoming', observation_id: 'obs_1', canonical_transaction_id: null, source_pipeline: 'gmail_transaction', merchant: 'Amazon.in', amount: -1499.00, date: '2026-06-10 14:32:00 UTC' },
            { id: 'm2', member_role: 'candidate_a', observation_id: null, canonical_transaction_id: 'tx_amazon', source_pipeline: 'statement_pdf', merchant: 'Amazon Pay', amount: -1499.00, date: '2026-06-10 14:32:00 IST' },
          ],
        },
        {
          id: 'cluster_2',
          reason: 'Near match: Amounts off by small margin',
          members_count: 3,
          members: [
            { id: 'm3', member_role: 'incoming', observation_id: 'obs_2', canonical_transaction_id: null, source_pipeline: 'gmail_transaction', merchant: 'Uber Trip', amount: -251.00, date: '2026-06-11' },
            { id: 'm4', member_role: 'candidate_a', observation_id: null, canonical_transaction_id: 'tx_uber_a', source_pipeline: 'statement_pdf', merchant: 'Uber', amount: -250.00, date: '2026-06-11' },
            { id: 'm5', member_role: 'candidate_b', observation_id: null, canonical_transaction_id: 'tx_uber_b', source_pipeline: 'statement_pdf', merchant: 'Uber.com', amount: -250.00, date: '2026-06-11' },
          ],
        },
      ];
      if (cmd === 'reconciliation_clusters_get') {
        return clusters.find((c) => c.id === args?.clusterId) || null;
      }
      return window.__MOCK_STATE__.no_clusters ? [] : clusters;
    }
    if (cmd === 'reconciliation_get_unassigned_transactions') {
      return window.__MOCK_STATE__.unassigned_transactions || [
        { id: 'u_1', observation_id: 'obs_orphan_1', reason: 'issuer_name_not_found', status: 'open', created_at: '2026-06-12T10:00:00Z' },
      ];
    }
    if (cmd === 'instruments_list') {
      return [
        { id: 'inst_1', instrument_type: 'credit_card', issuer_name: 'HDFC Bank', masked_identifier: '1234', status: 'active' },
        { id: 'inst_2', instrument_type: 'bank_account', issuer_name: 'ICICI Bank', masked_identifier: '5678', status: 'active' },
      ];
    }
    if (cmd === 'instruments_create') {
      if (window.__MOCK_STATE__.instrument_conflict) throw { code: 'CONSTRAINT_VIOLATION', message: 'An instrument with this identifier already exists for this issuer.' };
      return { id: 'inst_' + Date.now(), instrument_type: args.instrumentType, issuer_name: args.issuerName, masked_identifier: args.maskedIdentifier, status: 'active' };
    }
    if (cmd === 'instruments_update') return 'Updated';
    if (cmd === 'instruments_archive') {
      if (window.__MOCK_STATE__.instrument_delete_tied) throw { code: 'CONSTRAINT_VIOLATION', message: 'Cannot delete instrument with linked transactions.' };
      return 'Deleted';
    }
    if (cmd === 'get_debug_metrics') {
      return { total_transactions: 350, total_statements: 12, unresolved_clusters: 2, db_size_bytes: 4096000, app_version: '0.1.0' };
    }
    if (cmd === 'check_system_ram') return 16.0;
    if (cmd === 'llm_get_available_models') {
      // Doc 16 §12.3's 5-tier catalog, mirrored for tests.
      return [
        { id: 'gemma4_e4b', name: 'Gemma 4 E4B', tag: 'gemma4:e4b', tier: 1, min_ram_gb: 8.0, approx_size_gb: 5.0, rationale: 'Entry-level.', gguf_url: '', expected_sha256: '', tokenizer_url: null },
        { id: 'gemma4_12b', name: 'Gemma 4 12B', tag: 'gemma4:12b', tier: 2, min_ram_gb: 16.0, approx_size_gb: 9.0, rationale: 'Default.', gguf_url: '', expected_sha256: '', tokenizer_url: null },
        { id: 'qwen3_6_27b', name: 'Qwen3.6-27B (Dense)', tag: 'qwen3.6:27b', tier: 3, min_ram_gb: 16.0, approx_size_gb: 15.0, rationale: 'Alternative.', gguf_url: '', expected_sha256: '', tokenizer_url: null },
        { id: 'qwen3_6_35b_a3b', name: 'Qwen3.6-35B-A3B (MoE)', tag: 'qwen3.6:35b', tier: 4, min_ram_gb: 32.0, approx_size_gb: 21.0, rationale: 'Best case.', gguf_url: '', expected_sha256: '', tokenizer_url: null },
        { id: 'gemma4_31b', name: 'Gemma 4 31B (Dense)', tag: 'gemma4:31b', tier: 5, min_ram_gb: 32.0, approx_size_gb: 20.0, rationale: 'Max quality.', gguf_url: '', expected_sha256: '', tokenizer_url: null },
      ];
    }
    if (cmd === 'check_backend_status') return { status: 'healthy' };
    if (cmd === 'db_restore_backup') return 'ok';
    if (cmd === 'fetch_spending_limits') {
      return {
        global_limit: 60000,
        thresholds: { warn_at_80: true, warn_at_90: true, warn_at_100: true },
        categories: [
          { name: 'FOOD', budget: 8000 },
          { name: 'TRANSPORT', budget: 4000 },
          { name: 'SHOPPING', budget: 15000 },
          { name: 'ENTERTAINMENT', budget: 5000 }
        ],
      };
    }
    if (cmd === 'update_spending_limits') return 'ok';
    if (cmd === 'seed_mock_data') return 'Mock data seeded';
    if (cmd === 'is_gmail_connected') return window.__MOCK_STATE__.gmail_connected || false;
    if (cmd === 'list_connected_accounts' || cmd === 'settings_get_connected_accounts') {
      // TASK-FE-006 fix: the real command was renamed to
      // settings_get_connected_accounts (TASK-API-008) — this mock still
      // only had the pre-rename name, so GlobalStateContext's
      // refreshConnectedAccounts() (real callers, not just this test) always
      // fell through to the unhandled-command default ({}) in this fixture.
      if (window.__MOCK_STATE__.gmail_connected) {
        // TASK-FE-015 fix: account_status added -- ConnectedAccountsSettings
        // now renders a per-account status badge off this real field.
        return window.__MOCK_STATE__.connected_accounts || [
          { email: 'test@gmail.com', account_id: 'gmail_test_123', account_status: 'ACTIVE' },
        ];
      }
      return [];
    }
    if (cmd === 'auth_google_start') {
      if (window.__MOCK_STATE__.gmail_failure) throw { code: 'AUTH_FAILED', message: 'Failed to store token' };
      window.__MOCK_STATE__.gmail_connected = true;
      // TASK-FE-015: a (re)connect flips any degraded account back to
      // ACTIVE, so ConnectedAccountsSettings' "Reconnect" action is
      // actually drivable to completion in tests.
      if (window.__MOCK_STATE__.connected_accounts) {
        window.__MOCK_STATE__.connected_accounts = window.__MOCK_STATE__.connected_accounts.map((a) => ({ ...a, account_status: 'ACTIVE' }));
      }
      return 'oauth_completed';
    }
    if (cmd === 'auth_google_disconnect') {
      // TASK-FE-014 fix: no mock existed at all -- RevokeGmailButton's real
      // disconnect flow had never been driven through to completion.
      window.__MOCK_STATE__.gmail_connected = false;
      return undefined;
    }
    if (cmd === 'statements_submit_password') {
      // TASK-FE-012 fix: real backend resolves (never throws) with a
      // {status, statement_id, attempts_remaining?} object for both
      // wrong-password and success outcomes (see PasswordPromptModal's own
      // "I9 fix" comment) -- this mock threw and returned a bare string
      // 'success' instead, neither of which matches the real contract the
      // frontend code (old and new) has always been written against.
      if (args?.password === 'wrong' || window.__MOCK_STATE__.password_failure) {
        return { status: 'wrong_password', statement_id: args?.statementId, attempts_remaining: 2 };
      }
      return { status: 'unlocked', statement_id: args?.statementId };
    }
    if (cmd === 'statements_confirm_instrument') {
      return { status: 'confirmed', statement_id: args?.statementId };
    }
    if (cmd === 'statements_upload') {
      // TASK-FE-012 fix: no mock existed at all for the real batch-upload
      // command -- StatementUploadDropzone had never actually been driven
      // end-to-end (it always fell through to the {} unhandled-command
      // fallback, whose .results is undefined, silently skipping the
      // for-of loop over results with zero iterations).
      const files = args?.files || [];
      const results = files.map((f) => ({ status: 'ok', statement_id: 'stmt_new_' + Math.random().toString(36).slice(2), filename: f.filename }));
      return { results };
    }
    if (cmd === 'statements_list_unprocessed') {
      // TASK-FE-012 fix: no mock existed -- the real TASK-STMT-010 3-bucket
      // backend had zero frontend call sites (and therefore no mock) until
      // this task built UnprocessedItemsQueue against it.
      return window.__MOCK_STATE__.unprocessed_statements || { awaiting_password: [], pending_retry: [], failed: [] };
    }
    if (cmd === 'statements_retry_unprocessed') {
      return { status: 'retry_queued', statement_id: args?.statementId };
    }
    if (cmd === 'statements_discard') {
      return { status: 'discarded' };
    }
    if (cmd === 'reconciliation_clusters_resolve') {
      if (window.__MOCK_STATE__.resolve_failure) throw { code: 'NETWORK_ERROR', message: 'Resolution failed' };
      return 'resolved';
    }
    if (cmd === 'scans_historical') {
      return 'ok';
    }
    if (cmd === 'get_debug_metrics') return { total_transactions: 1000, total_statements: 50, unresolved_clusters: 12, db_size_bytes: 1024000, app_version: '1.0.0' };
    if (cmd === 'debug_get_pipeline_state') return { gmail_poll_paused: false, scan_queue_paused: true };
    if (cmd === 'check_system_ram') return 16.0;
    if (cmd === 'debug_fetch_audit_log') return [{ id: 'log-1', created_at: '2026-07-04T12:00:00Z', action: 'CLUSTER_RESOLVE', resource_type: 'Cluster', resource_id: 'cluster-1', actor_type: 'user', actor_id: 'user-1', after_json: { status: 'Resolved manually' } }];
    if (cmd === 'debug_fetch_pattern_rule_health') return [{ pattern_id: 'pat-1', regex: 'Uber.*', success_count: 50, failure_count: 0, last_used_at: '2026-07-04T12:00:00Z', created_at: '2026-07-01T12:00:00Z' }];
    if (cmd === 'debug_fetch_parse_errors') return [];
    if (cmd === 'debug_fetch_unprocessed_statements') return [];
    // G16 fix follow-up: Settings.tsx previously had no e2e coverage under
    // this fixture, so these mount-time calls silently fell through to the
    // default '{}' fallback below — {}.map() then crashed the whole page.
    if (cmd === 'license_get_status') {
      // TASK-FE-007 fix: state was lowercase ('active') and claimed an
      // already-paid subscription -- the real backend's LicenseStatusResponse
      // always uppercases state (compute_license_status / trial_status_response
      // in commands.rs), and a freshly onboarding test user is realistically
      // in TRIAL, not ACTIVE.
      return { state: 'TRIAL', is_active: true, license_key_masked: null, plan_id: null, billing_interval: null, expiry_date: '2026-07-30T00:00:00Z', days_remaining: 14 };
    }
    if (cmd === 'license_refresh') {
      // TASK-FE-016: the real command's own side effect (mirrored here so
      // LicenseLockOverlay/GracePeriodBanner's "retry" actions are actually
      // drivable to completion) is to emit a fresh license_state_changed
      // broadcast -- useLicenseStore mirrors that reactively, dismissing
      // the overlay/banner without any direct return-value handling.
      const status = window.__MOCK_STATE__.license_refresh_result || {
        state: 'ACTIVE', is_active: true, license_key_masked: null, plan_id: 'pro', billing_interval: 'monthly', expiry_date: null, days_remaining: null,
      };
      window.dispatchEvent(new CustomEvent('test-tauri-event', { detail: { event: 'license_state_changed', payload: status } }));
      return status;
    }
    if (cmd === 'auth_get_consent_history') {
      // TASK-FE-014: seeded with realistic data so ConsentHistoryList's
      // happy path (not just its empty state) is exercised by default.
      return window.__MOCK_STATE__.consent_history || [
        { id: 'consent_1', event_type: 'gmail_authorization', disclosure_text: 'Read-only access to Gmail to scan for financial emails.', consented_at: '2026-06-01T10:00:00Z', withdrawn_at: null },
        { id: 'consent_2', event_type: 'onboarding_network_disclosure', disclosure_text: 'Acknowledged the 5 outbound network destinations.', consented_at: '2026-06-01T09:58:00Z', withdrawn_at: null },
      ];
    }
    if (cmd === 'settings_pdf_passwords_list') {
      // TASK-FE-015: seeded with realistic data so StatementPasswordSettings'
      // happy path (not just its empty state) is exercised by default.
      return window.__MOCK_STATE__.pdf_passwords || [
        { id: 'pw_1', instrument_id: 'inst_1', issuer_name: 'HDFC Bank', masked_identifier: '1234', success_count: 3, last_used_at: '2026-06-01T10:00:00Z' },
      ];
    }
    if (cmd === 'settings_pdf_passwords_delete') {
      // TASK-FE-015 fix: the mock never mutated its own backing state, so a
      // "Forget" click never actually removed the row on refetch.
      if (window.__MOCK_STATE__.pdf_passwords) {
        window.__MOCK_STATE__.pdf_passwords = window.__MOCK_STATE__.pdf_passwords.filter((p) => p.id !== args?.id);
      } else {
        window.__MOCK_STATE__.pdf_passwords = [];
      }
      return undefined;
    }
    if (cmd === 'settings_pattern_rules_list') {
      // TASK-FE-014 fix: the real command is settings_pattern_rules_list
      // (TASK-API-008), returning a bare array -- no mock existed at all
      // (only the older debug-only debug_fetch_pattern_rule_health), so
      // this always fell through to the {} unhandled-command fallback and
      // Settings.tsx's patternRules.map(...) crashed with
      // "patternRules.map is not a function", which an ErrorBoundary
      // caught -- silently breaking the ENTIRE Settings page (not just the
      // Pattern Rules section) in every e2e test that ever visited
      // /#/settings, this whole session.
      // TASK-FE-018: a deliberate escape hatch for ErrorBoundary.spec.ts to
      // force a real render crash (mirrors the exact shape of the FE-014
      // bug above) rather than needing a synthetic React error trigger.
      if (window.__MOCK_STATE__.force_render_crash) return undefined;
      return window.__MOCK_STATE__.pattern_rules || [
        { id: 'pat_1', merchant_id: 'Uber', pattern_type: 'regex', pattern_value: 'Uber.*', is_active: true, success_count: 50, failure_count: 0 },
      ];
    }
    if (cmd === 'settings_pattern_rules_update') return undefined;
    if (cmd === 'record_consent_event') return undefined;
    if (cmd === 'export_logs') {
      // TASK-FE-014 fix: the real command resolves { success, file_path },
      // not a bare string -- PrivacySettings' "Export Diagnostic Bundle"
      // action reads result.file_path, which was always undefined against
      // this mock.
      return { success: true, file_path: '/Users/test/Library/Application Support/Dinero/diagnostic-bundle-2026-06-01.zip' };
    }
    if (cmd === 'auth_get_recovery_phrase') return 'test test test test test test test test test test test ball';
    if (cmd === 'settings_get_network_activity') {
      // TASK-FE-014 fix: the real command is settings_get_network_activity,
      // returning { entries: [...] } -- this mock was still on the stale
      // pre-rename settings_network_activity_list name returning a bare
      // array, so NetworkActivity.tsx's real call site always fell through
      // to the {} unhandled-command fallback (whose .entries is undefined,
      // crashing .then((r) => r.entries) into an unhandled rejection).
      if (window.__MOCK_STATE__.network_activity_failure) {
        throw { code: 'NETWORK_ERROR', message: 'Failed to fetch network activity.' };
      }
      return { entries: window.__MOCK_STATE__.network_activity || [] };
    }
    if (cmd === 'plugin:dialog|message') {
      // TASK-FE-011 fix: no mock existed at all -- @tauri-apps/plugin-dialog's
      // ask()/confirm()/message() all route through this single IPC command.
      // Every delete-confirmation flow in the app (TransactionQuickActions,
      // TransactionDetail, InstrumentDetail) calls this before deleting --
      // unmocked, it fell through to the {} fallback below and no test had
      // ever actually exercised a confirm-then-delete path end-to-end.
      // Real ask()/confirm() compare the resolved value against the OK-side
      // button's exact label string ("Yes"/"Ok"/a custom okLabel), not a
      // boolean (source: ask()'s === okLabel check in plugin-dialog's
      // dist-js) -- first attempt returning boolean true silently failed
      // this check (true === 'Yes' is false), so always resolve to the
      // OK-side label instead.
      const buttons = args?.buttons;
      if (buttons && typeof buttons === 'object' && 'ok' in buttons) return buttons.ok;
      if (buttons === 'OkCancel') return 'Ok';
      return 'Yes';
    }
    if (cmd === 'plugin:event|listen') {
      window.__TAURI_LISTENERS__ = window.__TAURI_LISTENERS__ || {};
      const { event, handler } = args;
      if (!window.__TAURI_LISTENERS__[event]) {
        window.__TAURI_LISTENERS__[event] = [];
      }
      window.__TAURI_LISTENERS__[event].push(handler);
      return Math.floor(Math.random() * 1000000);
    }
    
    // Default fallback
    console.warn('Unhandled mock command:', cmd);
    return {};
  };

  // Helper to emit events from tests, bridging Playwright dispatch to Tauri listeners
  window.addEventListener('test-tauri-event', (e) => {
    const detail = e.detail;
    const listeners = window.__TAURI_LISTENERS__?.[detail.event];
    if (listeners) {
      listeners.forEach(handlerId => {
         const func = window['_' + handlerId] || window[handlerId];
         if (typeof func === 'function') {
           func({ event: detail.event, id: Math.floor(Math.random() * 100000), payload: detail.payload });
         } else {
           console.error('Tauri mock: could not find function for handler', handlerId);
         }
      });
    }
  });

  window.localStorage.setItem('dinero_onboarded', 'true');
`;

export const test = base.extend({
  page: async ({ page }, use) => {
    // Inject Tauri mock into the page
    await page.addInitScript(tauriMockInitScript);
    // eslint-disable-next-line react-hooks/rules-of-hooks
    await use(page);
  },
});

export { expect } from '@playwright/test';
