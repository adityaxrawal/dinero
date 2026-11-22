/**
 * End-to-end workflows, each a numbered sequence of steps over the shared
 * component model. The workflow view renders these as an interactive sequence
 * and highlights the participating components on the system map.
 *
 * Sources: docs/design/00 §3 (master lifecycle), Documents 01–05.
 */
import type { WorkflowDef } from './types';
import { CONNECTIONS } from './connections';

const AUTHORED: WorkflowDef[] = [
  {
    id: 'boot-onboarding',
    name: 'Boot → Onboarding → First Scan',
    summary:
      'Fresh install: logging + DB + workers start, the onboarding gate redirects to the consent flow, Gmail is connected, and the historical backfill begins.',
    participants: [
      'react-ui',
      'onboarding-ui',
      'oauth',
      'historical-scan',
      'license-state',
      'sqlite',
    ],
    steps: [
      {
        id: 'boot-init',
        title: 'App boot',
        detail:
          'lib.rs::run initializes logging + panic hook, registers plugins, runs init_db (fails closed on error), manages shared state, spawns 12 background workers, then registers 142 IPC commands.',
        components: [
          'sqlite',
          'maintenance',
          'health-loop',
          'polling',
          'tx-queue',
          'stmt-queue',
          'mandate-queue',
          'layer6-queue',
          'ipc-bridge',
        ],
        relations: ['sync', 'db'],
      },
      {
        id: 'boot-check',
        title: 'Onboarding gate',
        detail:
          'WebView loads; AppLayout checks localStorage `dinero_onboarded`. Unset → navigate to /onboarding (replace).',
        components: ['react-ui', 'onboarding-ui'],
        relations: ['sync'],
      },
      {
        id: 'consent',
        title: 'Network disclosure + preferences',
        detail:
          'The user acknowledges NETWORK_DISCLOSURE_TABLE verbatim — recorded as a consent event before any network feature is reachable. Preferences (timezone, spending limit, LLM tier, statement preference) are saved to local_profile.',
        components: ['onboarding-ui', 'ipc-client', 'ipc-bridge', 'sqlite'],
        relations: ['sync', 'db'],
        outcomes: [
          { label: 'statement pref = auto', detail: 'Continue to Gmail consent' },
          { label: 'statement pref = manual', detail: 'Skip Gmail — jump to license step' },
        ],
      },
      {
        id: 'oauth-connect',
        title: 'Gmail OAuth (PKCE)',
        detail:
          'gmail_oauth_consent is recorded at button click, BEFORE the OAuth round-trip. startGoogle() runs the Authorization Code + PKCE flow with a loopback redirect (5-min timeout). Tokens land in macOS Keychain, one entry per account. A connected_accounts row is written on success.',
        components: ['onboarding-ui', 'oauth', 'google-oauth', 'keychain', 'sqlite'],
        relations: ['network', 'secret', 'db'],
        outcomes: [
          { label: 'success', detail: 'Advance to historical scan' },
          { label: 'failure', detail: 'mapOauthError shows a message; user stays on this step' },
        ],
      },
      {
        id: 'first-scan',
        title: 'Historical scan (background)',
        detail:
          'scans_historical claims a checkpoint atomically and returns "Scan started" immediately. run_scan_batches fetches up to 50 messages concurrently through Gate 1-2-3, checkpointing every 5. Progress arrives via scan_progress events.',
        components: [
          'historical-scan',
          'gmail-client',
          'message-processor',
          'extraction-ladder',
          'tx-queue',
          'stmt-queue',
          'mandate-queue',
          'events',
        ],
        relations: ['async', 'event', 'db'],
        outcomes: [
          {
            label: 'scan complete',
            detail: 'Emit scan_completed (note: Layer 6 enrichment may still be pending)',
          },
          {
            label: 'cancelled / failed',
            detail: 'Checkpoint marked accordingly; resume re-claims',
          },
        ],
      },
      {
        id: 'license-confirm',
        title: 'Trial confirmation → Dashboard',
        detail:
          'LicenseActivationScreen shows the auto-started 14-day trial (derived server-side from local_profile.created_at) or an existing paid state. `dinero_onboarded` was already set at the Gmail-consent transition. Continue navigates to the Dashboard.',
        components: ['onboarding-ui', 'license-state', 'react-ui'],
        relations: ['sync'],
      },
    ],
  },
  {
    id: 'historical-scan',
    name: 'Historical Scan (Bulk Backfill)',
    summary:
      'One-time per date range: search every message, fetch through Gate 1-2-3 at 50-way concurrency, checkpoint every 5, and route into the shared queues. Resumable across crashes.',
    participants: [
      'react-ui',
      'historical-scan',
      'gmail-client',
      'message-processor',
      'extraction-ladder',
      'tx-queue',
      'stmt-queue',
      'mandate-queue',
      'sqlite',
    ],
    steps: [
      {
        id: 'scan-start',
        title: 'Start scan (returns immediately)',
        detail:
          'scans_historical validates account_id + date range, passes the license write-gate, gets an OAuth token and atomically claims the checkpoint (one in_progress scan per account). It returns "Scan started" immediately — the scan runs as a detached task.',
        components: ['react-ui', 'historical-scan', 'license-gate', 'oauth', 'sqlite'],
        relations: ['sync', 'db'],
        outcomes: [
          { label: 'already running', detail: 'AppError::ScanAlreadyRunning — no spawn' },
          { label: 'claimed', detail: 'run_scan spawned in background' },
        ],
      },
      {
        id: 'scan-search',
        title: 'Search + paginate message IDs',
        detail:
          'GmailClient.search_messages over the date range with an on_page progress callback, capped at MAX_SEARCH_PAGES = 500. The initial checkpoint is written before processing begins.',
        components: ['historical-scan', 'gmail-client', 'sqlite'],
        relations: ['network', 'db'],
      },
      {
        id: 'scan-batch',
        title: 'Bounded fetch + process loop',
        detail:
          'Up to 50 concurrent full fetches (global semaphore shared with live polling), primed once and refilled one-for-one. Metadata-first gating keeps quota cost low. Every message runs the shared Gate 1-2-3 pipeline.',
        components: ['historical-scan', 'gmail-client', 'message-processor', 'extraction-ladder'],
        relations: ['sync', 'network'],
        outcomes: [
          { label: 'transaction', detail: '→ Transaction Queue' },
          { label: 'statement', detail: 'password resolve → Statement Queue' },
          { label: 'mandate', detail: '→ Mandate Queue' },
          { label: 'defer', detail: 'Layer 6 queue — counted as pending_enrichment' },
          { label: 'error', detail: 'scan_failed_messages row (durable); counted in errors' },
        ],
      },
      {
        id: 'scan-checkpoint',
        title: 'Checkpoint every 5 messages',
        detail:
          'upsert_checkpoint every 5 processed messages; cancellation is checked at the same cadence. The final checkpoint marks completed / cancelled / failed and the matching event is emitted.',
        components: ['historical-scan', 'sqlite', 'events'],
        relations: ['db', 'event'],
        outcomes: [
          {
            label: 'completed',
            detail: 'scan_completed — note: Layer 6 enrichment may still be pending',
          },
          { label: 'cancelled', detail: 'scan_cancelled; re-claim re-starts it' },
          { label: 'failed', detail: 'scan_failed; scans_resume re-claims' },
        ],
      },
      {
        id: 'scan-downstream',
        title: 'Downstream: queues → reconciliation',
        detail:
          'Transaction Queue normalizes + fingerprints + reconciles; Statement Queue feeds the statement pipeline; Mandate Queue upserts recurring payments. All three converge on the same reconciliation engine and ledger as live ingestion.',
        components: ['tx-queue', 'stmt-queue', 'mandate-queue', 'recon-engine', 'sqlite'],
        relations: ['async', 'db'],
      },
    ],
  },
  {
    id: 'steady-ingestion',
    name: 'Steady-State Ingestion → Ledger',
    summary:
      'Every 60s the poller pulls Gmail history, messages flow through Gate 1-2-3 into the queues, and reconciliation produces the canonical transactions the UI renders.',
    participants: [
      'polling',
      'gmail-client',
      'message-processor',
      'extraction-ladder',
      'tx-queue',
      'recon-engine',
      'sqlite',
      'react-ui',
    ],
    steps: [
      {
        id: 'poll-cycle',
        title: 'Poll cycle (60s / 300s)',
        detail:
          'The polling loop sleeps, checks GMAIL_POLL_PAUSED, then calls poll_all_accounts. The battery-aware loop (30s) recomputes the effective interval. Each active account is processed sequentially with per-account error isolation.',
        components: ['polling', 'battery-loop'],
        relations: ['sync'],
      },
      {
        id: 'history-sync',
        title: 'history.list since cursor',
        detail:
          'Cursor resolution: processing_checkpoints (job_type=gmail_history) first, then connected_accounts.last_history_id. No cursor → skip the account. 401 → force-refresh token once per cycle; 404/400 → clear cursor + history_checkpoint_reset audit entry; 429/5xx → backoff+jitter up to 8 retries.',
        components: ['polling', 'gmail-client', 'google-oauth', 'sqlite'],
        relations: ['network', 'db'],
        outcomes: [
          {
            label: 'new messages',
            detail: 'Each message goes through process_message (Gate 1-2-3)',
          },
          { label: 'no change', detail: 'Save new cursor only if a page returned a historyId' },
        ],
      },
      {
        id: 'gate-1',
        title: 'Gate 1 — sender verification',
        detail:
          'Registry match against verified_senders_registry.json plus homoglyph / punycode / typo-squat detection (Damerau-Levenshtein 0.85). Verified senders get an SPF/DKIM/DMARC cross-check that can only downgrade, never promote.',
        components: ['message-processor'],
        relations: ['sync'],
        outcomes: [
          { label: 'verified + auth pass', detail: 'Fetch full message → Gate 2' },
          { label: 'unverified, known history', detail: 'Rescued as "unknown bank"' },
          { label: 'reject / spoof', detail: 'Discarded (audited)' },
        ],
      },
      {
        id: 'gate-2',
        title: 'Gate 2 — content classification',
        detail:
          'Classify as TransactionAlert / BalanceUpdate / StatementEmail / Mandate* / Noise / Otp / Kyc / Marketing / Reminder / Unknown.',
        components: ['message-processor'],
        relations: ['sync'],
        outcomes: [
          { label: 'transaction', detail: 'Gate 3 extraction' },
          { label: 'statement', detail: 'Password resolve → Statement Queue' },
          { label: 'mandate', detail: 'Mandate extract → Mandate Queue' },
          { label: 'noise/unknown', detail: 'Ignored Messages (recoverable)' },
          { label: 'otp/kyc/marketing', detail: 'Audit Log (hard reject)' },
        ],
      },
      {
        id: 'gate-3',
        title: 'Gate 3 — extraction ladder',
        detail:
          'Layers 1→5 (learned rules → bank template → generic regex → NLP → statement cross-ref), cheapest first. A mandatory-field check rejects partial extractions. Ladder exhaustion on an LLM-eligible machine defers to the durable Layer 6 queue.',
        components: [
          'extraction-ladder',
          'learning',
          'merchant-norm',
          'fingerprint',
          'layer6-queue',
        ],
        relations: ['sync', 'async'],
        outcomes: [
          { label: 'pass', detail: 'Return transaction → Transaction Queue' },
          {
            label: 'defer',
            detail: 'Layer 6 queue (durable) — scan complete ≠ extraction complete',
          },
          { label: 'fail', detail: 'unassigned_transactions (partial data salvaged)' },
        ],
      },
      {
        id: 'queue-normalize',
        title: 'Normalize + fingerprint + observation',
        detail:
          'Transaction Queue workers (4, cap 256) build the observation row (normalize_observation), resolve its instrument and compute the event fingerprint, then hand it to reconciliation, whose insert is idempotent — UNIQUE(source_pipeline, source_record_id) makes re-runs safe. Merchant normalization already happened in the extraction ladder.',
        components: ['tx-queue', 'fingerprint', 'sqlite'],
        relations: ['db'],
      },
      {
        id: 'reconcile',
        title: 'Reconcile → canonical transaction',
        detail:
          'prefilter → exact_match → scorer → cluster. Ambiguous matches become reconciliation clusters for human review; the engine deliberately refuses to auto-resolve them. match_decisions records an immutable trail (append-only trigger).',
        components: ['recon-engine', 'sqlite'],
        relations: ['db', 'sync'],
      },
      {
        id: 'ui-update',
        title: 'transaction_created event → UI',
        detail: 'The React dashboard updates from the emitted event and React Query invalidation.',
        components: ['tx-queue', 'events', 'react-ui'],
        relations: ['event'],
      },
    ],
  },
  {
    id: 'statement-commit',
    name: 'Statement → Reviewed → Committed',
    summary:
      'A PDF statement — from email attachment or manual upload — is parsed in an isolated sidecar, gated on instrument metadata, staged as an editable draft, and only becomes real transactions after explicit user commit.',
    participants: [
      'statement-pipeline',
      'pdf-sidecar',
      'instrument-gate',
      'stmt-queue',
      'sqlite',
      'react-ui',
      'recon-engine',
    ],
    steps: [
      {
        id: 'intake',
        title: 'Intake (email or upload)',
        detail:
          'Manual: statements_upload validates (non-empty, type, size, SHA-256) and checks file-hash + filename billing-cycle duplicates. Email: the Gate 2 statement branch calls the same resolve_statement_password choke point and enqueues the same StatementJob.',
        components: [
          'react-ui',
          'ipc-client',
          'ipc-bridge',
          'stmt-queue',
          'statement-pipeline',
          'message-processor',
        ],
        relations: ['sync'],
        outcomes: [
          { label: 'duplicate', detail: 'DUPLICATE_REJECTED + audit entry; batch continues' },
          { label: 'new file', detail: 'Password resolution' },
        ],
      },
      {
        id: 'password',
        title: 'Password resolution',
        detail:
          'is_pdf_unencrypted → try_all_stored_passwords (ordered by success_count/last_used_at) → PromptCreated. A wrong password re-prompts without losing context. awaiting_password NEVER blocks — the caller moves to the next file.',
        components: ['statement-pipeline', 'pdf-storage', 'pdf-sidecar'],
        relations: ['sync', 'pipe', 'db'],
        outcomes: [
          { label: 'resolved / unencrypted', detail: 'Queue statement job' },
          {
            label: 'parked',
            detail: 'unprocessed_statements (awaiting_password) — re-parse later',
          },
        ],
      },
      {
        id: 'parse',
        title: 'Parse + metadata (5-permit queue)',
        detail:
          'Under the 5-permit semaphore: store encrypted PDF bytes → sidecar extract_text (empty pages = hard parse_failed) → extract issuer/masked-ID/billing period/due date. ParseMethod fallback chain: Pdfium → OcrFallback (Tesseract) → LlmAssist.',
        components: ['stmt-queue', 'statement-pipeline', 'pdf-sidecar', 'pdf-storage'],
        relations: ['pipe', 'db'],
      },
      {
        id: 'instrument',
        title: 'Instrument Gate',
        detail:
          'Issuer + masked-ID are MANDATORY — never defaulted. Missing metadata parks the statement as awaiting_instrument and emits INSTRUMENT_CONFIRMATION_REQUIRED. resolve_or_create_instrument runs an atomic INSERT OR IGNORE on UNIQUE(type, issuer_name, masked_identifier).',
        components: ['instrument-gate', 'sqlite'],
        relations: ['gate', 'db'],
        outcomes: [
          { label: 'blocked', detail: 'awaiting_instrument — user confirms the instrument' },
          { label: 'passes', detail: 'Save working password → duplicate check' },
        ],
      },
      {
        id: 'dedup-rows',
        title: 'Duplicate check + row extraction',
        detail:
          'check_billing_cycle_duplicate (authoritative, post-metadata) rejects and audits duplicates. Row extraction runs per-bank parsers then parse_universal; pages with no deterministic rows fall back to row_llm with a verbatim-quote hallucination guard.',
        components: ['statement-pipeline', 'row-extractor', 'llm-manager'],
        relations: ['sync'],
      },
      {
        id: 'stage',
        title: 'Stage draft (pending_review)',
        detail:
          'statement_drafts row with status pending_review is written and statement_staged is emitted. The user can edit rows and metadata before anything becomes real.',
        components: ['statement-pipeline', 'sqlite', 'events'],
        relations: ['db', 'event'],
      },
      {
        id: 'review',
        title: 'User review + commit',
        detail:
          'commit_staged_draft asserts the draft is still pending_review (hard error otherwise), writes statements + statement_entries, builds observations, inserts them idempotently and reconciles each row. Per-observation reconciliation failure is logged and does not abort. bill_classifier derives the upcoming bill.',
        components: ['react-ui', 'statement-pipeline', 'recon-engine', 'sqlite'],
        relations: ['sync', 'db'],
        outcomes: [
          {
            label: 'commit',
            detail: 'Committed statement id returned; entries reconcile into the ledger',
          },
          { label: 'discard', detail: 'Draft and stored encrypted PDF removed' },
        ],
      },
    ],
  },
  {
    id: 'ipc-command',
    name: 'IPC Command Dispatch (Write-Gate)',
    summary:
      'Every mutating command validates arguments, passes the fail-closed license write-gate, dispatches, and reports long-running outcomes through named events.',
    participants: [
      'react-ui',
      'ipc-client',
      'ipc-validation',
      'license-gate',
      'ipc-bridge',
      'events',
    ],
    steps: [
      {
        id: 'invoke',
        title: 'invoke(command, args)',
        detail:
          'The React layer calls through the typed lib/ipc.ts boundary — every backend call funnels through it.',
        components: ['react-ui', 'ipc-client'],
        relations: ['sync'],
      },
      {
        id: 'validate',
        title: 'Validation + session middleware',
        detail:
          'ipc::validation runs before any DB/network call for commands accepting user-supplied identifiers or date ranges. ipc::middleware::require_active_session gates session-dependent commands.',
        components: ['ipc-validation'],
        relations: ['sync'],
        outcomes: [
          { label: 'invalid', detail: 'AppError::Validation — command never starts' },
          { label: 'valid', detail: 'Proceed to write-gate' },
        ],
      },
      {
        id: 'gate-check',
        title: 'assert_write_allowed',
        detail:
          'Fail-closed license check: the cached status is never trusted alone — the backing JWT is cryptographically re-verified and device-bound. Invalid JWT is treated as Locked. 37 write commands are test-enforced to call this.',
        components: ['license-gate', 'license-state'],
        relations: ['gate'],
        outcomes: [
          { label: 'Locked', detail: 'Write refused (reads remain available)' },
          { label: 'Ok', detail: 'Dispatch to the command handler' },
        ],
      },
      {
        id: 'dispatch',
        title: 'Dispatch + immediate ack',
        detail:
          'The command handler runs and returns an immediate result. Expensive work (scans, statement parsing) is spawned and acknowledged right away.',
        components: ['ipc-bridge'],
        relations: ['sync', 'async'],
      },
      {
        id: 'emit',
        title: 'Events + store update',
        detail:
          'Named events (scan_progress, statement_staged, transaction_created …) reach the UI through listen() callbacks, which invalidate React Query caches and update Zustand stores.',
        components: ['events', 'react-ui'],
        relations: ['event'],
      },
    ],
  },
  {
    id: 'layer6-enrichment',
    name: 'Layer 6 Deferred LLM Enrichment',
    summary:
      'When the deterministic ladder fails, the message is parked in a durable queue and a local llama-server extracts it off the hot path — so a slow inference never stalls a scan.',
    participants: [
      'extraction-ladder',
      'layer6-queue',
      'llm-manager',
      'llama-sidecar',
      'tx-queue',
      'sqlite',
    ],
    steps: [
      {
        id: 'defer',
        title: 'Ladder exhausted → enqueue',
        detail:
          'Layers 1–5 all failed and the machine is LLM-eligible. The observation + unassigned_transactions row are ALREADY persisted (partial data salvaged); process_message enqueues a Layer6Job and returns EnqueuedForEnrichment.',
        components: ['extraction-ladder', 'layer6-queue', 'sqlite'],
        relations: ['async', 'db'],
      },
      {
        id: 'durable',
        title: 'Durable queue drain',
        detail:
          'layer6_pending_jobs is the only queue with a durable backing table, so jobs survive app restart. 6 workers (cap 256) are over-spawned relative to the llama-sidecar semaphore, which actually gates inference.',
        components: ['layer6-queue', 'sqlite'],
        relations: ['db'],
      },
      {
        id: 'infer',
        title: 'llama-server inference',
        detail:
          'llm_manager routes the prompt to the loopback-bound llama-server (pinned llama.cpp b10068). Context size scales with parallel slots (2048 × slots).',
        components: ['llm-manager', 'llama-sidecar'],
        relations: ['pipe'],
      },
      {
        id: 'apply',
        title: 'Apply result',
        detail:
          'Success: fill the event time, complete the observation and route through the Transaction Queue canonical path. Failure: leave it in unassigned_transactions for user review.',
        components: ['layer6-queue', 'tx-queue', 'sqlite'],
        relations: ['db', 'async'],
        outcomes: [
          { label: 'pass', detail: 'Observation completed → canonical transaction' },
          { label: 'fail', detail: 'unassigned_transactions for manual resolution' },
        ],
      },
    ],
  },
  {
    id: 'licensing-revalidation',
    name: 'Licensing Revalidation & Offline Grace',
    summary:
      'A 6h background tick re-validates the device-bound JWT. Unreachable backend degrades to Grace with a 7-day offline window; clock rollback beyond 1 hour hard-locks.',
    participants: [
      'license-worker',
      'licensing-backend',
      'license-state',
      'license-gate',
      'events',
    ],
    steps: [
      {
        id: 'tick',
        title: '6h validation tick',
        detail:
          'licensing::worker calls the backend at https://api.dinero-app.com. The license itself is a device-bound RS256 JWT — the app holds only the public key.',
        components: ['license-worker', 'network-client', 'licensing-backend'],
        relations: ['network'],
      },
      {
        id: 'apply-transition',
        title: 'Apply legal transition',
        detail:
          'is_legal_transition enforces exactly 8 edges. The same-status transition is a no-op. PastDue/Canceled/Expired parse from the server but participate in NO local transition.',
        components: ['license-state'],
        relations: ['sync', 'db'],
        outcomes: [
          { label: 'valid', detail: 'Active (or Trial / AnonymousEval preserved)' },
          {
            label: 'backend unreachable',
            detail: 'Active → Grace; 7-day offline grace from last_server_validated_at',
          },
          { label: 'grace exhausted', detail: 'Grace → Locked' },
          {
            label: 'clock rollback > 1h',
            detail: '→ Locked with ClockSkewDetected + license_clock_skew event',
          },
        ],
      },
      {
        id: 'gate-effect',
        title: 'Write-gate effect',
        detail:
          'Locked blocks every mutating command (reads stay available). Escaping Locked requires license_activate establishing wholesale fresh server-verified state — a local edge back to Active would be a trivial tamper.',
        components: ['license-gate', 'license-state', 'events'],
        relations: ['gate', 'event'],
      },
    ],
  },
  {
    id: 'incident-response',
    name: 'Incident Response Convergence',
    summary:
      'Three independent failure signals converge on the IncidentMonitor, which responds with session revocation, Gmail disconnect and health alerts.',
    participants: ['oauth', 'maintenance', 'health-loop', 'incident-monitor', 'events'],
    steps: [
      {
        id: 'triggers',
        title: 'Failure triggers',
        detail:
          'RepeatedOAuthFailure (from oauth.rs mark_account_degraded_async), IntegrityCheckFailure (daily check — fires on FIRST occurrence), BackendStartupFailure (60s health loop SELECT 1).',
        components: ['oauth', 'maintenance', 'health-loop', 'incident-monitor'],
        relations: ['async'],
      },
      {
        id: 'respond',
        title: 'Threshold → respond',
        detail:
          'Per-trigger-kind thresholds; crossing one runs the response: session revoke, Gmail disconnect, health alert. Backup verification failure raises a system_warning instead of an incident.',
        components: ['incident-monitor', 'events'],
        relations: ['event'],
      },
    ],
  },
];

/**
 * Lanes and relation badges are derived rather than trusted: every component a
 * step names gets a lane (authored lanes keep their order), and a step's
 * relation badges are the kinds of the connections it actually exercises.
 * Steps internal to one component keep their authored relation.
 */
function normalise(w: WorkflowDef): WorkflowDef {
  const participants = [...new Set([...w.participants, ...w.steps.flatMap((s) => s.components)])];
  const steps = w.steps.map((s) => {
    const inStep = new Set(s.components);
    const kinds = [
      ...new Set(
        CONNECTIONS.filter((c) => inStep.has(c.from) && inStep.has(c.to)).map((c) => c.kind)
      ),
    ];
    return { ...s, relations: kinds.length > 0 ? kinds : s.relations };
  });
  return { ...w, participants, steps };
}

export const WORKFLOWS: WorkflowDef[] = AUTHORED.map(normalise);

export const WORKFLOW_BY_ID: Record<string, WorkflowDef> = Object.fromEntries(
  WORKFLOWS.map((w) => [w.id, w])
);
