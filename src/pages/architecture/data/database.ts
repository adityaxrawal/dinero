/**
 * Verified database model — 43 tables built by applying all 60 migrations
 * (recounted 2026-08-01). Only the architecturally significant entities and
 * their relationships are modelled here; the five failure/deferral tables and
 * the two immutability triggers are called out explicitly because they change
 * how you may write against the schema.
 */
import type { EntityDef, EntityRelation } from './types';

export const ENTITIES: EntityDef[] = [
  {
    id: 'local_profile',
    name: 'local_profile',
    group: 'identity',
    description: 'Single-row install profile. created_at drives the 14-day trial window.',
    columns: [
      { name: 'id', type: 'INTEGER', key: 'PK' },
      { name: 'created_at', type: 'DATETIME', note: 'drives trial window' },
      { name: 'timezone', type: 'TEXT' },
      { name: 'spending_limit_monthly', type: 'INTEGER' },
      { name: 'historical_scan_months', type: 'INTEGER' },
      { name: 'llm_model', type: 'TEXT' },
      { name: 'statement_preference', type: 'TEXT', note: 'auto | manual' },
    ],
  },
  {
    id: 'connected_accounts',
    name: 'connected_accounts',
    group: 'identity',
    description:
      'Gmail accounts connected via OAuth. account_status follows ACTIVE / degraded / disconnected; degraded only after a permanent auth failure (reconnect needed), never for transient errors.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'profile_id', type: 'INTEGER', key: 'FK' },
      { name: 'email_address', type: 'TEXT' },
      { name: 'account_status', type: 'TEXT' },
      { name: 'last_history_id', type: 'TEXT', note: 'polling cursor fallback' },
      { name: 'created_at', type: 'DATETIME' },
    ],
  },
  {
    id: 'consent_events',
    name: 'consent_events',
    group: 'identity',
    description: 'Verbatim consent disclosures. Never deleted — only withdrawn_at is set.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      {
        name: 'event_type',
        type: 'TEXT',
        note: 'network_disclosure_acknowledged, gmail_oauth_consent',
      },
      { name: 'disclosure_text', type: 'TEXT', note: 'verbatim, not a summary' },
      { name: 'consented_at', type: 'DATETIME' },
      { name: 'withdrawn_at', type: 'DATETIME', note: 'nullable' },
    ],
  },
  {
    id: 'license_state',
    name: 'license_state',
    group: 'identity',
    description:
      'Cached license status + JWT. Re-verified and device-bound on every write-gate check.',
    columns: [
      { name: 'subscription_status_cached', type: 'TEXT' },
      { name: 'license_jwt', type: 'TEXT' },
      { name: 'last_server_validated_at', type: 'DATETIME', note: '7-day offline grace anchor' },
    ],
  },
  {
    id: 'processing_checkpoints',
    name: 'processing_checkpoints',
    group: 'ops',
    description:
      'Resumable job state keyed (job_type, job_key) — historical scans and Gmail history cursors.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'job_type', type: 'TEXT', key: 'UQ', note: 'historical_scan | gmail_history' },
      { name: 'job_key', type: 'TEXT', key: 'UQ' },
      { name: 'checkpoint_state_json', type: 'TEXT' },
      { name: 'last_processed_token', type: 'TEXT' },
      { name: 'status', type: 'TEXT' },
    ],
  },
  {
    id: 'instruments',
    name: 'instruments',
    group: 'ledger',
    description:
      'Cards/accounts. UNIQUE(type, issuer_name, masked_identifier) enables atomic resolve.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'type', type: 'TEXT', note: 'CHECK, 12 values' },
      { name: 'issuer_name', type: 'TEXT', key: 'UQ' },
      { name: 'masked_identifier', type: 'TEXT', key: 'UQ' },
    ],
  },
  {
    id: 'transactions',
    name: 'transactions',
    group: 'ledger',
    description: 'The canonical ledger. amount is a generated virtual column — display only.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'unique_event_id', type: 'TEXT', key: 'UQ' },
      { name: 'amount_minor', type: 'INTEGER', note: 'all arithmetic uses this' },
      {
        name: 'amount',
        type: 'REAL',
        key: 'V',
        note: 'GENERATED ALWAYS AS (amount_minor/100.0) VIRTUAL — unwritable',
      },
      { name: 'parent_transaction_id', type: 'TEXT', key: 'FK', note: 'refunds / reversals' },
      { name: 'channel', type: 'TEXT' },
      { name: 'notes', type: 'TEXT' },
    ],
  },
  {
    id: 'transaction_observations',
    name: 'transaction_observations',
    group: 'ledger',
    description:
      'Raw extracted events. UNIQUE(source_pipeline, source_record_id) makes ingestion idempotent.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      {
        name: 'source_pipeline',
        type: 'TEXT',
        note: 'CHECK: gmail_transaction | statement_pdf | manual',
      },
      { name: 'source_record_id', type: 'TEXT', key: 'UQ' },
      { name: 'fingerprint', type: 'TEXT' },
      { name: 'extraction_method', type: 'TEXT' },
      { name: 'statement_id', type: 'TEXT', key: 'FK' },
    ],
  },
  {
    id: 'statement_drafts',
    name: 'statement_drafts',
    group: 'statements',
    description: 'Staged, user-editable statement rows. Commit mints a real statement id.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'file_hash', type: 'TEXT' },
      { name: 'instrument_id', type: 'TEXT', key: 'FK' },
      { name: 'issuer_name', type: 'TEXT' },
      { name: 'masked_identifier', type: 'TEXT' },
      { name: 'rows_json', type: 'TEXT' },
      { name: 'status', type: 'TEXT', note: 'pending_review …' },
    ],
  },
  {
    id: 'statements',
    name: 'statements',
    group: 'statements',
    description: 'Committed statements. parse_status CHECK: queued | processing | parsed | failed.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'source_type', type: 'TEXT', note: 'gmail_email | manual_upload' },
      { name: 'file_hash', type: 'TEXT' },
      { name: 'parse_status', type: 'TEXT' },
    ],
  },
  {
    id: 'statement_entries',
    name: 'statement_entries',
    group: 'statements',
    description: 'Rows of a committed statement.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'statement_id', type: 'TEXT', key: 'FK' },
    ],
  },
  {
    id: 'unprocessed_statements',
    name: 'unprocessed_statements',
    group: 'statements',
    description: 'Parked statements awaiting user input. One of five failure/deferral tables.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'statement_source_json', type: 'TEXT' },
      { name: 'failure_type', type: 'TEXT', note: 'password_required | instrument_unresolved' },
      {
        name: 'status',
        type: 'TEXT',
        note: 'awaiting_password | awaiting_instrument_confirmation',
      },
    ],
  },
  {
    id: 'pdf_passwords',
    name: 'pdf_passwords',
    group: 'statements',
    description:
      'Learned per-instrument PDF passwords, AES-256-GCM ciphertext. Never returned to the UI.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'instrument_id', type: 'TEXT', key: 'FK' },
      { name: 'password_ciphertext', type: 'BLOB' },
      { name: 'success_count', type: 'INTEGER' },
      { name: 'last_used_at', type: 'DATETIME' },
    ],
  },
  {
    id: 'unassigned_transactions',
    name: 'unassigned_transactions',
    group: 'reconciliation',
    description: 'Extraction failures with salvaged partial data, awaiting user resolution.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'observation_id', type: 'TEXT', key: 'FK' },
      { name: 'reason', type: 'TEXT', note: 'extraction_failed' },
      { name: 'status', type: 'TEXT' },
    ],
  },
  {
    id: 'reconciliation_clusters',
    name: 'reconciliation_clusters',
    group: 'reconciliation',
    description: 'Ambiguous match groups the engine refuses to auto-resolve.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'cluster_status', type: 'TEXT', note: 'open | resolved | deferred | rejected' },
    ],
  },
  {
    id: 'reconciliation_cluster_members',
    name: 'reconciliation_cluster_members',
    group: 'reconciliation',
    description: 'Membership of an ambiguous cluster.',
    columns: [
      { name: 'cluster_id', type: 'TEXT', key: 'FK' },
      { name: 'observation_id', type: 'TEXT', key: 'FK' },
      { name: 'canonical_transaction_id', type: 'TEXT', key: 'FK' },
    ],
  },
  {
    id: 'match_decisions',
    name: 'match_decisions',
    group: 'reconciliation',
    description: 'Immutable decision trail — BEFORE UPDATE/DELETE trigger makes it append-only.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'observation_id', type: 'TEXT', key: 'FK' },
      { name: 'matched_transaction_id', type: 'TEXT', key: 'FK' },
    ],
  },
  {
    id: 'layer6_pending_jobs',
    name: 'layer6_pending_jobs',
    group: 'ops',
    description: 'Durable backing for deferred LLM work — the only queue that survives restart.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'observation_id', type: 'TEXT', note: 'logical ref, no FK constraint' },
    ],
  },
  {
    id: 'scan_failed_messages',
    name: 'scan_failed_messages',
    group: 'ops',
    description: 'Durable per-message scan failure record (a record, not a work queue).',
    columns: [
      { name: 'account_id', type: 'TEXT', note: 'logical ref, no FK constraint' },
      { name: 'msg_id', type: 'TEXT' },
      { name: 'error', type: 'TEXT' },
      { name: 'failed_at', type: 'DATETIME' },
    ],
  },
  {
    id: 'network_activity_log',
    name: 'network_activity_log',
    group: 'ops',
    description:
      'On-device audit trail of outbound calls through NetworkClient (3 of 5 channels — ADR-024).',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'channel', type: 'TEXT' },
    ],
  },
  {
    id: 'audit_log',
    name: 'audit_log',
    group: 'ops',
    description: 'Append-only audit trail — BEFORE UPDATE/DELETE trigger enforced by SQLite.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'action', type: 'TEXT' },
      { name: 'row_hash', type: 'TEXT', note: 'hash chain with prev_hash' },
    ],
  },
  {
    id: 'merchants',
    name: 'merchants',
    group: 'ledger',
    description: 'Normalized merchant identities; confidence is scored on read, not stored.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'name', type: 'TEXT' },
    ],
  },
  {
    id: 'field_rules',
    name: 'field_rules',
    group: 'ledger',
    description:
      'Learned extraction rules (Layer 1 of the ladder) with per-template-shape variants. Variant status: pending → active (3 successes) → trusted (10); inactive after 3 failures or a hit rate below 70%. Pending (LLM-drift) rules are promoted by shadow evaluation against committed Layer 6 results.',
    columns: [{ name: 'id', type: 'TEXT', key: 'PK' }],
  },
];

/**
 * `logical: true` marks a relationship the schema does not enforce: either the
 * column has no FOREIGN KEY constraint, or the link is a lifecycle step with no
 * stored reference. These render dashed. Everything else is a real FK
 * (verified with pragma_foreign_key_list against all 60 migrations).
 */
export const ENTITY_RELATIONS: EntityRelation[] = [
  { from: 'local_profile', to: 'connected_accounts', cardinality: '||--o{', label: 'has' },
  {
    from: 'local_profile',
    to: 'license_state',
    cardinality: '||--||',
    label: 'governs (singletons)',
    logical: true,
  },
  {
    from: 'local_profile',
    to: 'consent_events',
    cardinality: '||--o{',
    label: 'records',
    logical: true,
  },
  {
    from: 'connected_accounts',
    to: 'processing_checkpoints',
    cardinality: '||--o{',
    label: 'tracks (job_key)',
    logical: true,
  },
  {
    from: 'connected_accounts',
    to: 'scan_failed_messages',
    cardinality: '||--o{',
    label: 'per-message errors',
    logical: true,
  },
  { from: 'instruments', to: 'transactions', cardinality: '||--o{', label: 'owns' },
  { from: 'instruments', to: 'statements', cardinality: '||--o{', label: 'owns' },
  { from: 'instruments', to: 'pdf_passwords', cardinality: '||--o{', label: 'learned' },
  { from: 'instruments', to: 'statement_drafts', cardinality: '||--o{', label: 'instrument_id' },
  {
    from: 'statement_drafts',
    to: 'statements',
    cardinality: '||--o|',
    label: 'commit mints',
    logical: true,
  },
  { from: 'statements', to: 'statement_entries', cardinality: '||--o{', label: 'contains' },
  { from: 'statements', to: 'transaction_observations', cardinality: '||--o{', label: 'source' },
  {
    from: 'statements',
    to: 'unprocessed_statements',
    cardinality: '||--o{',
    label: 'resolved from',
  },
  {
    from: 'instruments',
    to: 'transaction_observations',
    cardinality: '||--o{',
    label: 'observed on',
  },
  {
    from: 'statement_entries',
    to: 'transaction_observations',
    cardinality: '||--o|',
    label: 'produces',
  },
  // FK is transaction_observations.canonical_transaction_id → transactions
  {
    from: 'transactions',
    to: 'transaction_observations',
    cardinality: '||--o{',
    label: 'canonical for',
  },
  {
    from: 'transaction_observations',
    to: 'unassigned_transactions',
    cardinality: '||--o|',
    label: 'if failed',
  },
  {
    from: 'transaction_observations',
    to: 'layer6_pending_jobs',
    cardinality: '||--o{',
    label: 'deferred LLM job',
    logical: true,
  },
  { from: 'transactions', to: 'match_decisions', cardinality: '||--o{', label: 'immutable trail' },
  {
    from: 'transaction_observations',
    to: 'match_decisions',
    cardinality: '||--o{',
    label: 'decided for',
  },
  {
    from: 'reconciliation_clusters',
    to: 'reconciliation_cluster_members',
    cardinality: '||--o{',
    label: 'groups',
  },
  {
    from: 'transactions',
    to: 'reconciliation_cluster_members',
    cardinality: '||--o{',
    label: 'if ambiguous',
  },
  {
    from: 'transaction_observations',
    to: 'reconciliation_cluster_members',
    cardinality: '||--o{',
    label: 'member',
  },
  {
    from: 'merchants',
    to: 'transactions',
    cardinality: '||--o{',
    label: 'normalizes (merchant_entity_id)',
    logical: true,
  },
  {
    from: 'field_rules',
    to: 'transaction_observations',
    cardinality: '||--o{',
    label: 'extracts via',
    logical: true,
  },
];

/** Schema facts that change how you write queries. */
export const SCHEMA_FACTS = [
  {
    fact: 'transactions.amount is GENERATED ALWAYS AS (amount_minor / 100.0) VIRTUAL',
    consequence: 'Display only, unwritable. All arithmetic and matching uses amount_minor.',
  },
  {
    fact: 'audit_log and match_decisions carry BEFORE UPDATE / DELETE triggers',
    consequence: 'Append-only is enforced by SQLite itself, not by code review.',
  },
  {
    fact: 'instruments has UNIQUE(type, issuer_name, masked_identifier)',
    consequence:
      "What resolve_instrument()'s atomic INSERT OR IGNORE depends on to avoid duplicates.",
  },
  {
    fact: 'transaction_observations.source_pipeline is CHECK-constrained',
    consequence:
      'Only gmail_transaction / statement_pdf / manual — a fourth source needs a migration.',
  },
  {
    fact: 'Five failure/deferral tables exist',
    consequence:
      'unassigned_transactions, unprocessed_statements, scan_failed_messages, layer6_pending_jobs, network_activity_log. There is no generic dead-letter table.',
  },
];
