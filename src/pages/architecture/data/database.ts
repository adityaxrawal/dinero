/**
 * Verified database model — every table built by applying every migration
 * (counts in ./counts.ts; model.test.ts fails if a table is missing here, audit
 * N-G3). The five failure/deferral tables and the two immutability triggers are
 * called out explicitly because they change how you may write against the
 * schema.
 */
import type { EntityDef, EntityRelation } from './types';

export const ENTITIES: EntityDef[] = [
  {
    id: 'local_profile',
    name: 'local_profile',
    group: 'identity',
    description:
      'Single-row install profile, created at startup (startup fails if it cannot be). created_at is the offline fallback for the 14-day trial; the trial is anchored on the licensing server.',
    columns: [
      { name: 'id', type: 'INTEGER', key: 'PK' },
      { name: 'created_at', type: 'DATETIME', note: 'offline trial fallback' },
      { name: 'timezone', type: 'TEXT' },
      { name: 'spending_limit_monthly', type: 'INTEGER' },
      { name: 'historical_scan_months', type: 'INTEGER' },
      { name: 'llm_model', type: 'TEXT' },
      { name: 'statement_preference', type: 'TEXT', note: 'auto | manual' },
      {
        name: 'onboarded_at',
        type: 'DATETIME',
        note: 'onboarding gate; set by the preferences save',
      },
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
      {
        name: 'last_history_id',
        type: 'TEXT',
        note: 'connect-time cursor seed; the gmail_history checkpoint is the cursor of record (both written together)',
      },
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
      {
        name: 'job_type',
        type: 'TEXT',
        key: 'UQ',
        note: 'historical_scan | gmail_history | gmail_catch_up',
      },
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
      'Raw extracted events. UNIQUE(source_pipeline, source_record_id) makes ingestion idempotent; a partial UNIQUE on rfc822_message_id books one email delivered to two connected inboxes once.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      {
        name: 'source_pipeline',
        type: 'TEXT',
        note: 'CHECK: gmail_transaction | statement_pdf | manual',
      },
      { name: 'source_record_id', type: 'TEXT', key: 'UQ' },
      {
        name: 'rfc822_message_id',
        type: 'TEXT',
        key: 'UQ',
        note: 'email Message-ID; unique when present',
      },
      { name: 'fingerprint', type: 'TEXT' },
      { name: 'extraction_method', type: 'TEXT' },
      { name: 'statement_id', type: 'TEXT', key: 'FK' },
    ],
  },
  {
    id: 'statement_drafts',
    name: 'statement_drafts',
    group: 'statements',
    description:
      'Staged, user-editable statement rows. Commit claims the draft (committing) and mints a real statement id in one transaction.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'file_hash', type: 'TEXT' },
      { name: 'instrument_id', type: 'TEXT', key: 'FK' },
      { name: 'issuer_name', type: 'TEXT' },
      { name: 'masked_identifier', type: 'TEXT' },
      { name: 'rows_json', type: 'TEXT' },
      { name: 'status', type: 'TEXT', note: 'pending_review | committing | committed …' },
      {
        name: 'missing_pages',
        type: 'TEXT',
        note: 'JSON page numbers; commit needs acknowledge_partial',
      },
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
      { name: 'source_filename', type: 'TEXT', note: 'kept for queued-statement recovery' },
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
    description: 'Parked statements awaiting user input. One of the failure/deferral tables.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'statement_source_json', type: 'TEXT' },
      { name: 'failure_type', type: 'TEXT', note: 'password_required | instrument_unresolved' },
      {
        name: 'status',
        type: 'TEXT',
        note: 'awaiting_password | awaiting_instrument_confirmation',
      },
      { name: 'pdf_retained_until', type: 'TEXT', note: 'awaiting-password PDFs kept 7 days' },
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
    description: 'Durable backing for deferred LLM work, so jobs survive restart.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'observation_id', type: 'TEXT', note: 'logical ref, no FK constraint' },
      {
        name: 'attempts',
        type: 'INTEGER',
        note: 'failed attempts, paces retries; never dead (legacy dead_at rows only)',
      },
      { name: 'dead_at', type: 'TEXT' },
    ],
  },
  {
    id: 'pending_ingest_jobs',
    name: 'pending_ingest_jobs',
    group: 'ops',
    description:
      'Durable backing for transaction and mandate jobs from Gmail: written before the job is queued, deleted once processed, replayed at launch.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK', note: 'transaction:<msg> | mandate:<msg>' },
      { name: 'kind', type: 'TEXT' },
      { name: 'payload_json', type: 'TEXT' },
      { name: 'attempts', type: 'INTEGER', note: 'dead after 3' },
      { name: 'dead_at', type: 'TEXT' },
    ],
  },
  {
    id: 'processed_messages',
    name: 'processed_messages',
    group: 'ops',
    description:
      'One row per Gmail message already decided (accepted, rejected or mandate), so rescans skip it.',
    columns: [
      { name: 'message_id', type: 'TEXT', key: 'PK' },
      { name: 'decision', type: 'TEXT' },
      { name: 'created_at', type: 'TEXT' },
    ],
  },
  {
    id: 'active_system_warnings',
    name: 'active_system_warnings',
    group: 'ops',
    description: 'System warnings still active, reloaded at launch and deleted when cleared.',
    columns: [
      { name: 'warning_type', type: 'TEXT', key: 'PK' },
      { name: 'payload_json', type: 'TEXT' },
      { name: 'updated_at', type: 'TEXT' },
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
      'On-device audit trail of outbound calls (Gmail, Google OAuth, licensing, model downloads); only the updater check is missing (ADR-024). Kept 30 days.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'channel', type: 'TEXT' },
    ],
  },
  {
    id: 'audit_log',
    name: 'audit_log',
    group: 'ops',
    description:
      'Append-only audit trail — BEFORE UPDATE/DELETE trigger enforced by SQLite. Kept indefinitely.',
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
      'Learned extraction rules (Layer 1 of the ladder) with per-template-shape variants. Variant status: pending → active (3 successes) → trusted (10); inactive after 3 failures or a hit rate below 70%. One variant serves a template (active/trusted) with at most one pending challenger beside it; a challenger replaces the serving variant only by a better shadow hit rate, never one learned from a user correction.',
    columns: [{ name: 'id', type: 'TEXT', key: 'PK' }],
  },
  {
    id: 'recurring_payments',
    name: 'recurring_payments',
    group: 'ledger',
    description:
      'Recurring payments and mandates, one row per instrument + merchant. Mandate events and recurring detection upsert it; mandates never book a ledger transaction.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'instrument_id', type: 'TEXT', key: 'FK' },
      { name: 'merchant_entity_id', type: 'TEXT', key: 'FK' },
      { name: 'amount_minor', type: 'BIGINT' },
      { name: 'cadence', type: 'TEXT' },
      { name: 'next_predicted_date', type: 'DATE' },
      { name: 'status', type: 'TEXT' },
      { name: 'source', type: 'TEXT', note: 'inferred (detector) | explicit (mandate)' },
      { name: 'external_mandate_id', type: 'TEXT' },
    ],
  },
  {
    id: 'unresolved_mandate_cancellations',
    name: 'unresolved_mandate_cancellations',
    group: 'ledger',
    description:
      'Mandate-cancellation emails that matched no single recurring payment, held for the user to resolve.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'raw_signal', type: 'TEXT' },
      { name: 'candidate_ids', type: 'TEXT' },
      { name: 'status', type: 'TEXT' },
    ],
  },
  {
    id: 'categories',
    name: 'categories',
    group: 'ledger',
    description:
      'The fixed spending taxonomy transactions are classified into, with optional monthly budgets.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'parent_id', type: 'TEXT', key: 'FK' },
      { name: 'name', type: 'TEXT' },
      { name: 'monthly_budget_minor', type: 'INTEGER' },
      { name: 'is_deleted', type: 'BOOLEAN' },
    ],
  },
  {
    id: 'tags',
    name: 'tags',
    group: 'ledger',
    description: 'User-defined tags.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'name', type: 'TEXT' },
      { name: 'color_hex', type: 'TEXT' },
    ],
  },
  {
    id: 'transaction_tags',
    name: 'transaction_tags',
    group: 'ledger',
    description: 'Tag ↔ transaction links.',
    columns: [
      { name: 'transaction_id', type: 'TEXT', key: 'PK' },
      { name: 'tag_id', type: 'TEXT', key: 'PK' },
    ],
  },
  {
    id: 'merchant_aliases',
    name: 'merchant_aliases',
    group: 'ledger',
    description:
      'Raw merchant strings mapped to a merchant entity (normalisation and merchant cleanup).',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'merchant_entity_id', type: 'TEXT', key: 'FK' },
      { name: 'alias_raw', type: 'TEXT' },
      { name: 'alias_normalized', type: 'TEXT' },
      { name: 'confidence', type: 'REAL' },
    ],
  },
  {
    id: 'merchant_llm_corrections',
    name: 'merchant_llm_corrections',
    group: 'ledger',
    description:
      'Before/after record of every merchant-cleanup change, so a cleanup run can be reverted.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'run_id', type: 'TEXT' },
      { name: 'transaction_id', type: 'TEXT', key: 'FK' },
      { name: 'prev_merchant_entity_id', type: 'TEXT' },
      { name: 'new_merchant_entity_id', type: 'TEXT' },
      { name: 'status', type: 'TEXT' },
    ],
  },
  {
    id: 'field_rule_variants',
    name: 'field_rule_variants',
    group: 'reconciliation',
    description:
      'Learned extraction rule versions: one serving variant per rule, challengers evaluated in shadow until they beat it (user edits take over at once).',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'field_rule_id', type: 'TEXT', key: 'FK' },
      { name: 'template_hash', type: 'TEXT' },
      { name: 'status', type: 'TEXT', note: 'pending | active | trusted | inactive | flagged' },
      { name: 'success_count', type: 'INTEGER' },
      { name: 'failure_count', type: 'INTEGER' },
      { name: 'authored_by', type: 'TEXT' },
    ],
  },
  {
    id: 'rule_change_log',
    name: 'rule_change_log',
    group: 'reconciliation',
    description: 'Audit trail of learned-rule changes and the feedback that triggered them.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'field_rule_variant_id', type: 'TEXT', key: 'FK' },
      { name: 'action', type: 'TEXT' },
      { name: 'triggering_feedback_log_id', type: 'TEXT', key: 'FK' },
      { name: 'reason', type: 'TEXT' },
    ],
  },
  {
    id: 'feedback_log',
    name: 'feedback_log',
    group: 'reconciliation',
    description:
      'Field corrections the user made; the input to rule learning and sender overrides.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'transaction_id', type: 'TEXT', key: 'FK' },
      { name: 'observation_id', type: 'TEXT', key: 'FK' },
      { name: 'field_name', type: 'TEXT' },
      { name: 'old_value', type: 'TEXT' },
      { name: 'new_value', type: 'TEXT' },
    ],
  },
  {
    id: 'ignored_messages',
    name: 'ignored_messages',
    group: 'ops',
    description:
      'Gate 2 noise/unknown messages, kept 30 days so they can be recovered; rescans skip them.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'message_id', type: 'TEXT' },
      { name: 'bank_name', type: 'TEXT' },
      { name: 'reason', type: 'TEXT' },
      { name: 'subject', type: 'TEXT' },
      { name: 'expires_at', type: 'DATETIME', note: '30 days' },
    ],
  },
  {
    id: 'sender_reputation',
    name: 'sender_reputation',
    group: 'ops',
    description: 'Per-domain sighting counters that feed the "new sender" suggestions.',
    columns: [
      { name: 'domain', type: 'TEXT', key: 'PK' },
      { name: 'message_count', type: 'INTEGER' },
      { name: 'verified_pass_count', type: 'INTEGER' },
      { name: 'last_verification_result', type: 'TEXT' },
    ],
  },
  {
    id: 'pending_senders',
    name: 'pending_senders',
    group: 'ops',
    description:
      'Unregistered sender domains offered to the user; an approved one is read but its results go to review.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'domain', type: 'TEXT' },
      { name: 'bank_name', type: 'TEXT' },
      { name: 'status', type: 'TEXT', note: 'pending | approved' },
      { name: 'reject_count', type: 'INTEGER' },
    ],
  },
  {
    id: 'sender_bank_overrides',
    name: 'sender_bank_overrides',
    group: 'ops',
    description:
      'User corrections of which bank a sender domain belongs to ("wrong bank" reports).',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'domain', type: 'TEXT' },
      { name: 'bank_name', type: 'TEXT' },
      { name: 'status', type: 'TEXT' },
      { name: 'triggering_feedback_log_id', type: 'TEXT', key: 'FK' },
    ],
  },
  {
    id: 'instrument_rules',
    name: 'instrument_rules',
    group: 'ops',
    description:
      'User-confirmed "always use this card" rules for alerts that name the bank and card type but no card number.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'issuer_name', type: 'TEXT' },
      { name: 'instrument_type', type: 'TEXT' },
      { name: 'instrument_id', type: 'TEXT', key: 'FK' },
      { name: 'status', type: 'TEXT' },
    ],
  },
  {
    id: 'alerts',
    name: 'alerts',
    group: 'ops',
    description: 'Threshold alerts raised by the alert worker (e.g. an ambiguous cluster waiting).',
    columns: [
      { name: 'alert_id', type: 'TEXT', key: 'PK' },
      { name: 'type', type: 'TEXT' },
      { name: 'related_cluster_id', type: 'TEXT', key: 'FK' },
      { name: 'status', type: 'TEXT' },
    ],
  },
  {
    id: 'dismissed_system_warnings',
    name: 'dismissed_system_warnings',
    group: 'ops',
    description:
      'System warnings the user dismissed, keyed by type and message hash so a changed warning shows again.',
    columns: [
      { name: 'warning_type', type: 'TEXT', key: 'PK' },
      { name: 'message_hash', type: 'TEXT' },
      { name: 'dismissed_at', type: 'DATETIME' },
    ],
  },
  {
    id: 'sync_metadata',
    name: 'sync_metadata',
    group: 'ops',
    description: 'Last sync time per bank, for the missing-data alerts.',
    columns: [
      { name: 'bank_name', type: 'TEXT', key: 'PK' },
      { name: 'last_synced_at', type: 'DATETIME' },
    ],
  },
  {
    id: 'release_readiness_snapshots',
    name: 'release_readiness_snapshots',
    group: 'ops',
    description: 'Locally captured quality-gate metrics (release go/no-go), never transmitted.',
    columns: [
      { name: 'id', type: 'TEXT', key: 'PK' },
      { name: 'captured_at', type: 'DATETIME' },
      { name: 'metrics_json', type: 'TEXT' },
      { name: 'go_no_go', type: 'INTEGER' },
    ],
  },
];

/**
 * `logical: true` marks a relationship the schema does not enforce: either the
 * column has no FOREIGN KEY constraint, or the link is a lifecycle step with no
 * stored reference. These render dashed. Everything else is a real FK
 * (verified with pragma_foreign_key_list against every migration).
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
      'What the one shared resolver (get_or_create_instrument, canonical issuer + cleaned masked id) depends on to avoid duplicates; archived rows hold their key, so an archived card is reported, never duplicated.',
  },
  {
    fact: 'transaction_observations.source_pipeline is CHECK-constrained',
    consequence:
      'Only gmail_transaction / statement_pdf / manual — a fourth source needs a migration.',
  },
  {
    fact: 'Five failure/deferral tables exist',
    consequence:
      'unassigned_transactions, unprocessed_statements, scan_failed_messages, layer6_pending_jobs, pending_ingest_jobs. pending_ingest_jobs marks a job dead after 3 attempts (a dead ingest job is listed in scan_failed_messages); layer6_pending_jobs never does, since its failures are infrastructure, and an open review entry with a live job stays hidden from the queue; there is no generic dead-letter table. At most one open review entry per observation (partial unique index); unassigned and draft statuses are CHECK-constrained.',
  },
];
