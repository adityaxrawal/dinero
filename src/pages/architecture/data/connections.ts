/**
 * Directed relationships between components.
 *
 * `kind` drives the visual treatment of the edge (solid = sync call, dashed =
 * async/event, dotted = store access). `workflows` tags which end-to-end flows
 * exercise the connection so the workflow views can highlight exactly the
 * relevant subgraph.
 */
import type { ConnectionDef } from './types';

export const CONNECTIONS: ConnectionDef[] = [
  // ── client ⇄ ipc ──────────────────────────────────────────────────────────
  {
    id: 'ui-ipc',
    from: 'react-ui',
    to: 'ipc-client',
    kind: 'sync',
    label: 'typed invoke()',
  },
  {
    id: 'onb-ipc',
    from: 'onboarding-ui',
    to: 'ipc-client',
    kind: 'sync',
    label: 'consent / prefs / OAuth',
  },
  {
    id: 'ipc-bridge',
    from: 'ipc-client',
    to: 'ipc-bridge',
    kind: 'sync',
    label: 'invoke(command, args)',
  },
  {
    id: 'bridge-valid',
    from: 'ipc-bridge',
    to: 'ipc-validation',
    kind: 'sync',
    label: 'validate args',
  },
  {
    id: 'valid-gate',
    from: 'ipc-bridge',
    to: 'license-gate',
    kind: 'gate',
    label: 'assert_write_allowed (39 writes)',
    detail: 'Every mutating command passes the write-gate before touching the DB or network.',
  },
  {
    id: 'gate-events',
    from: 'license-state',
    to: 'events',
    kind: 'event',
    label: 'license_state_changed',
  },
  {
    id: 'bridge-events',
    from: 'ipc-bridge',
    to: 'events',
    kind: 'event',
    label: 'progress / completion',
    detail: 'Expensive ops acknowledge immediately and report outcomes via named events.',
  },
  {
    id: 'events-ui',
    from: 'events',
    to: 'react-ui',
    kind: 'event',
    label: 'listen() callback',
  },

  // ── onboarding ────────────────────────────────────────────────────────────
  {
    id: 'onb-ui',
    from: 'react-ui',
    to: 'onboarding-ui',
    kind: 'sync',
    label: 'not onboarded',
  },
  {
    id: 'bridge-oauth',
    from: 'ipc-bridge',
    to: 'oauth',
    kind: 'sync',
    label: 'auth_google_start',
  },
  {
    id: 'bridge-scan',
    from: 'ipc-bridge',
    to: 'historical-scan',
    kind: 'async',
    label: 'scans_historical',
  },
  {
    id: 'bridge-lic',
    from: 'ipc-bridge',
    to: 'license-state',
    kind: 'sync',
    label: 'license_get_status',
  },

  // ── ingestion: oauth + gmail ─────────────────────────────────────────────
  {
    id: 'oauth-net',
    from: 'oauth',
    to: 'network-client',
    kind: 'sync',
    label: 'audited token request',
    detail: 'Token exchange and refresh use a NetworkClient transport for the oauth2 crate.',
  },
  {
    id: 'oauth-google',
    from: 'network-client',
    to: 'google-oauth',
    kind: 'network',
    label: 'PKCE token exchange / refresh',
    detail: 'Logged under the google_oauth channel.',
  },
  {
    id: 'oauth-keychain',
    from: 'oauth',
    to: 'keychain',
    kind: 'secret',
    label: 'store tokens',
    detail: 'One isolated Keychain entry per account under com.dinero.app.',
  },
  {
    id: 'oauth-client',
    from: 'oauth',
    to: 'gmail-client',
    kind: 'sync',
    label: 'get_valid_access_token',
  },
  {
    id: 'client-gmail',
    from: 'network-client',
    to: 'gmail-api',
    kind: 'network',
    label: 'messages / history',
    detail: 'All calls routed through NetworkClient for the on-device audit trail.',
  },
  {
    id: 'client-net',
    from: 'gmail-client',
    to: 'network-client',
    kind: 'sync',
    label: 'audited request',
  },
  {
    id: 'net-store',
    from: 'network-client',
    to: 'sqlite',
    kind: 'db',
    label: 'network_activity_log',
  },

  // ── ingestion: entry points → gating ────────────────────────────────────
  {
    id: 'scan-client',
    from: 'historical-scan',
    to: 'gmail-client',
    kind: 'sync',
    label: 'search + fetch',
  },
  {
    id: 'scan-mp',
    from: 'historical-scan',
    to: 'message-processor',
    kind: 'sync',
    label: 'process_message',
  },
  {
    id: 'poll-client',
    from: 'polling',
    to: 'gmail-client',
    kind: 'sync',
    label: 'history.list',
  },
  {
    id: 'poll-mp',
    from: 'polling',
    to: 'message-processor',
    kind: 'sync',
    label: 'process_message',
    detail: 'One processing path per job type regardless of entry point.',
  },
  {
    id: 'poll-ckpt',
    from: 'polling',
    to: 'sqlite',
    kind: 'db',
    label: 'historyId cursor',
  },
  {
    id: 'scan-ckpt',
    from: 'historical-scan',
    to: 'sqlite',
    kind: 'db',
    label: 'checkpoints + failures',
  },
  {
    id: 'mp-ladder',
    from: 'message-processor',
    to: 'extraction-ladder',
    kind: 'sync',
    label: 'Gate 3 extract',
  },

  // ── extraction ───────────────────────────────────────────────────────────
  {
    id: 'ladder-merchant',
    from: 'extraction-ladder',
    to: 'merchant-norm',
    kind: 'sync',
    label: 'normalize merchant',
  },
  {
    id: 'ladder-fp',
    from: 'extraction-ladder',
    to: 'fingerprint',
    kind: 'sync',
    label: 'fingerprint',
  },
  {
    id: 'ladder-learn',
    from: 'extraction-ladder',
    to: 'learning',
    kind: 'sync',
    label: 'Layer 1 rules',
    detail: 'Learned field rules are the cheapest rung of the ladder.',
  },
  {
    id: 'ladder-l6',
    from: 'message-processor',
    to: 'layer6-queue',
    kind: 'async',
    label: 'defer (Layers 1–5 fail)',
  },
  {
    id: 'l6-llm',
    from: 'layer6-queue',
    to: 'llm-manager',
    kind: 'sync',
    label: 'infer',
  },
  {
    id: 'llm-sidecar',
    from: 'llm-manager',
    to: 'llama-sidecar',
    kind: 'sync', // loopback HTTP request/response, not a stdio pipe (llama_sidecar.rs)
    label: 'llama-server HTTP',
    detail: 'Loopback-bound llama-server on 127.0.0.1 with allowed-origin lock.',
  },
  {
    id: 'llm-cdn',
    from: 'llm-manager',
    to: 'model-cdn',
    kind: 'network',
    label: 'GGUF + binary (logged)',
    detail:
      'Streamed downloads (15 s connect / 60 s read timeouts) recorded with log_unpooled_request under the llm_download channel.',
  },
  {
    id: 'llm-netlog',
    from: 'llm-manager',
    to: 'network-client',
    kind: 'sync',
    label: 'log_unpooled_request',
  },
  {
    id: 'learn-llm',
    from: 'learning',
    to: 'llm-manager',
    kind: 'sync',
    label: 'author rules',
  },
  {
    id: 'learn-db',
    from: 'learning',
    to: 'sqlite',
    kind: 'db',
    label: 'field_rules',
  },
  {
    id: 'merchant-llm',
    from: 'merchant-norm',
    to: 'llm-manager',
    kind: 'sync',
    label: 'batch cleanup',
  },

  // ── queues → reconciliation ──────────────────────────────────────────────
  {
    // ingestion/queues.rs process_transaction_job
    id: 'txq-fp',
    from: 'tx-queue',
    to: 'fingerprint',
    kind: 'sync',
    label: 'compute_fingerprint',
  },
  {
    id: 'txq-db',
    from: 'tx-queue',
    to: 'sqlite',
    kind: 'db',
    label: 'resolve_instrument',
  },
  {
    id: 'txq-recon',
    from: 'tx-queue',
    to: 'recon-engine',
    kind: 'sync',
    label: 'reconcile',
  },
  {
    id: 'l6-txq',
    from: 'layer6-queue',
    to: 'tx-queue',
    kind: 'async',
    label: 'enriched observation',
  },
  {
    id: 'l6-unassigned',
    from: 'layer6-queue',
    to: 'sqlite',
    kind: 'db',
    label: 'unassigned on failure',
  },
  {
    id: 'stmtq-parse',
    from: 'stmt-queue',
    to: 'statement-pipeline',
    kind: 'sync',
    label: 'stage_parse_pipeline',
  },
  {
    id: 'scan-stmtq',
    from: 'historical-scan',
    to: 'stmt-queue',
    kind: 'async',
    label: 'statement job',
  },
  {
    id: 'poll-stmtq',
    from: 'polling',
    to: 'stmt-queue',
    kind: 'async',
    label: 'statement job',
  },
  {
    id: 'scan-txq',
    from: 'historical-scan',
    to: 'tx-queue',
    kind: 'async',
    label: 'transaction job',
  },
  {
    id: 'poll-txq',
    from: 'polling',
    to: 'tx-queue',
    kind: 'async',
    label: 'transaction job',
  },
  {
    id: 'scan-mandateq',
    from: 'historical-scan',
    to: 'mandate-queue',
    kind: 'async',
    label: 'mandate job',
  },
  {
    id: 'poll-mandateq',
    from: 'polling',
    to: 'mandate-queue',
    kind: 'async',
    label: 'mandate job',
  },
  {
    id: 'upload-stmtq',
    from: 'ipc-bridge', // statements_upload → upload_one_statement enqueues the job
    to: 'stmt-queue',
    kind: 'async',
    label: 'manual upload job',
  },
  {
    id: 'mandateq-recur',
    from: 'mandate-queue',
    to: 'mandate-recurring',
    kind: 'sync',
    label: 'recurring upsert',
  },

  // ── statements ───────────────────────────────────────────────────────────
  {
    id: 'stmt-sidecar',
    from: 'statement-pipeline',
    to: 'pdf-sidecar',
    kind: 'pipe',
    label: 'unlock_check / extract_text',
    detail: 'PDF bytes travel over the pipe only — never written to disk during sidecar calls.',
  },
  {
    id: 'stmt-gate',
    from: 'statement-pipeline',
    to: 'instrument-gate',
    kind: 'gate',
    label: 'Instrument Gate',
  },
  {
    // commands/mod.rs: a blocked statement is parked as
    // unprocessed_statements.status = 'awaiting_instrument_confirmation'
    id: 'instgate-db',
    from: 'instrument-gate',
    to: 'sqlite',
    kind: 'db',
    label: 'park awaiting confirmation',
  },
  {
    id: 'stmt-rows',
    from: 'statement-pipeline',
    to: 'row-extractor',
    kind: 'sync',
    label: 'extract rows',
  },
  {
    id: 'rows-llm',
    from: 'row-extractor',
    to: 'llm-manager',
    kind: 'sync',
    label: 'row_llm fallback',
    detail: 'Every LLM row must quote amount and description verbatim (hallucination guard).',
  },
  {
    id: 'stmt-pdf',
    from: 'statement-pipeline',
    to: 'pdf-storage',
    kind: 'db',
    label: 'encrypted PDF + password',
  },
  {
    id: 'stmt-draft',
    from: 'statement-pipeline',
    to: 'sqlite',
    kind: 'db',
    label: 'draft / entries / observations',
  },
  {
    id: 'stmt-recon',
    from: 'statement-pipeline',
    to: 'recon-engine',
    kind: 'sync',
    label: 'reconcile each row',
    detail: 'Per-observation failure is logged and does not abort the commit.',
  },
  {
    id: 'pdfstorage-keychain',
    from: 'pdf-storage',
    to: 'keychain',
    kind: 'secret',
    label: 'derive AES key',
    detail: 'Domain-separated from the SQLCipher DB key (PDF_KEY_DOMAIN_LABEL).',
  },

  // ── reconciliation ───────────────────────────────────────────────────────
  {
    id: 'recon-db',
    from: 'recon-engine',
    to: 'sqlite',
    kind: 'db',
    label: 'canonical write + trail',
  },
  {
    id: 'recon-alert',
    from: 'recon-engine',
    to: 'recon-alert',
    kind: 'async',
    label: 'threshold alerts',
  },
  {
    id: 'alert-events',
    from: 'recon-alert',
    to: 'events',
    kind: 'event',
    label: 'AlertThresholdCrossed',
  },
  {
    // Emitted by the Transaction Queue worker after ingest_observation
    // (ingestion/queues.rs), not by the reconciliation engine itself.
    id: 'txq-events',
    from: 'tx-queue',
    to: 'events',
    kind: 'event',
    label: 'transaction_created / reconciliation_cluster',
  },

  // ── licensing ────────────────────────────────────────────────────────────
  {
    id: 'gate-licstate',
    from: 'license-gate',
    to: 'license-state',
    kind: 'sync',
    label: 'verify JWT + device',
    detail:
      'Cached status is never trusted alone; the JWT is re-verified and device-bound on every check.',
  },
  {
    id: 'licstate-db',
    from: 'license-state',
    to: 'sqlite',
    kind: 'db',
    label: 'license_state',
  },
  {
    id: 'licworker-backend',
    from: 'network-client',
    to: 'licensing-backend',
    kind: 'network',
    label: 'validate / refresh',
  },
  {
    id: 'licworker-net',
    from: 'license-worker',
    to: 'network-client',
    kind: 'sync',
    label: 'audited request',
  },
  {
    id: 'licworker-state',
    from: 'license-worker',
    to: 'license-state',
    kind: 'sync',
    label: 'apply transition',
  },
  {
    id: 'licstate-keychain',
    from: 'license-state',
    to: 'keychain',
    kind: 'secret',
    label: 'JWT verification key',
  },

  // ── persistence / workers ────────────────────────────────────────────────
  {
    // The UI never touches SQLite: command handlers query it and return the
    // result over the IPC response.
    id: 'bridge-db',
    from: 'ipc-bridge',
    to: 'sqlite',
    kind: 'db',
    label: 'command queries / writes',
  },
  {
    id: 'backup-db',
    from: 'backup',
    to: 'sqlite',
    kind: 'db',
    label: 'VACUUM INTO + verify',
  },
  {
    id: 'maint-backup',
    from: 'maintenance',
    to: 'backup',
    kind: 'sync',
    label: 'daily backup',
  },
  {
    id: 'maint-db',
    from: 'maintenance',
    to: 'sqlite',
    kind: 'db',
    label: 'integrity / retention / archival',
  },
  {
    id: 'health-db',
    from: 'health-loop',
    to: 'sqlite',
    kind: 'db',
    label: 'SELECT 1',
  },
  {
    id: 'health-incident',
    from: 'health-loop',
    to: 'incident-monitor',
    kind: 'async',
    label: 'BackendStartupFailure',
  },
  {
    id: 'oauth-incident',
    from: 'oauth',
    to: 'incident-monitor',
    kind: 'async',
    label: 'RepeatedOAuthFailure',
  },
  {
    id: 'maint-incident',
    from: 'maintenance',
    to: 'incident-monitor',
    kind: 'async',
    label: 'IntegrityCheckFailure',
  },
  {
    id: 'licworker-incident',
    from: 'license-worker',
    to: 'incident-monitor',
    kind: 'async',
    label: 'licence validate / signature failures',
  },
  {
    id: 'incident-events',
    from: 'incident-monitor',
    to: 'events',
    kind: 'event',
    label: 'respond + warn',
  },
  {
    id: 'updater-cdn',
    from: 'updater',
    to: 'model-cdn',
    kind: 'network',
    label: 'release check (unaudited)',
    detail: 'tauri-plugin-updater bypasses NetworkClient — ADR-024.',
  },
  {
    id: 'menu-db',
    from: 'menu-tray',
    to: 'sqlite',
    kind: 'db',
    label: 'summary counts',
  },
  {
    id: 'battery-poll',
    from: 'battery-loop',
    to: 'polling',
    kind: 'sync',
    label: 'interval (60s / 300s)',
  },
  {
    id: 'poll-net',
    from: 'polling',
    to: 'network-client',
    kind: 'sync',
    label: 'audited HTTP',
  },
];

/** Helper: map component id → all connections touching it. */
export function connectionsFor(componentId: string): ConnectionDef[] {
  return CONNECTIONS.filter((c) => c.from === componentId || c.to === componentId);
}
