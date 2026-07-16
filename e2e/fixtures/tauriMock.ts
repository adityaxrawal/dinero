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
  };

  window.__TAURI_INTERNALS__.invoke = async function (cmd, args) {
    if (cmd === 'dashboard_summary') {
      return { total_spend: 2199.0, income: 110000.0, upcoming_bills: 2, limit: 60000.0 };
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
    // Pre-existing gap (found during the G20/H10/J8 rename): unmocked, so
    // availableTags resolved to the default '{}' fallback instead of an
    // array, crashing the detail panel's tag autocomplete
    // (availableTags.filter().map()) on every row click.
    if (cmd === 'tags_list') return ['online', 'dinner', 'work', 'personal'];
    if (cmd === 'transactions_update') {
      if (window.__MOCK_STATE__.tx_update_failure) throw { code: 'UPDATE_FAILED', message: 'Failed to save update' };
      return 'Updated';
    }
    if (cmd === 'statements_list') {
      return [
        { id: 'stmt_1', date: '2026-06-01T10:00:00Z', file_name: 'HDFC_May_2026.pdf', status: 'PROCESSED' },
        { id: 'stmt_2', date: '2026-05-01T10:00:00Z', file_name: 'HDFC_Apr_2026.pdf', status: 'PROCESSED' },
        { id: 'stmt_3', date: '2026-06-05T12:00:00Z', file_name: 'ICICI_May_2026.pdf', status: 'PASSWORD_REQUIRED' },
      ];
    }
    if (cmd === 'reconciliation_clusters_list') {
      return [
        { 
          id: 'cluster_1', 
          reason: 'Ambiguous match: Same amount on same day', 
          members_count: 2,
          members: [
            { id: 'm1', source: 'Bank Sync', merchant: 'Amazon Pay', amount: -1499.00, date: '2026-06-10 14:32:00 IST' },
            { id: 'm2', source: 'Gmail Parser', merchant: 'Amazon.in', amount: -1499.00, date: '2026-06-10 14:32:00 UTC' }
          ]
        },
        { 
          id: 'cluster_2', 
          reason: 'Near match: Amounts off by small margin', 
          members_count: 3,
          members: [
            { id: 'm3', source: 'Bank Sync', merchant: 'Uber', amount: -250.00, date: '2026-06-11' },
            { id: 'm4', source: 'Gmail Parser', merchant: 'Uber Trip', amount: -251.00, date: '2026-06-11' },
            { id: 'm5', source: 'Gmail Parser', merchant: 'Uber.com', amount: -250.00, date: '2026-06-11' }
          ]
        },
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
        return [{ email: 'test@gmail.com', account_id: 'gmail_test_123' }];
      }
      return [];
    }
    if (cmd === 'auth_google_start') {
      if (window.__MOCK_STATE__.gmail_failure) throw { code: 'AUTH_FAILED', message: 'Failed to store token' };
      window.__MOCK_STATE__.gmail_connected = true;
      return 'oauth_completed';
    }
    if (cmd === 'statements_submit_password') {
      if (args?.password === 'wrong' || window.__MOCK_STATE__.password_failure) {
        throw { code: 'INVALID_PASSWORD', message: 'Incorrect password. Please try again.' };
      }
      return 'success';
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
    if (cmd === 'auth_get_consent_history') return [];
    if (cmd === 'settings_pdf_passwords_list') return [];
    if (cmd === 'settings_pdf_passwords_delete') return undefined;
    if (cmd === 'pattern_rule_set_status') return undefined;
    if (cmd === 'record_consent_event') return undefined;
    if (cmd === 'export_logs') return 'ok';
    if (cmd === 'auth_get_recovery_phrase') return 'test test test test test test test test test test test ball';
    if (cmd === 'settings_network_activity_list') return [];
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
