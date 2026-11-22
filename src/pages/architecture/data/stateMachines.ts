/**
 * State machines verified against source (2026-08-01).
 *
 * The license machine is the one that matters most for correctness: the design
 * docs contain BOTH an "aspirational" graph and the graph the code actually
 * enforces. Only the enforced one is modelled here, with the difference called
 * out in `notes`.
 */
import type { StateMachineDef } from './types';

export const STATE_MACHINES: StateMachineDef[] = [
  {
    id: 'license',
    name: 'License State Machine',
    summary:
      'The locally enforced transition graph — exactly 8 edges. Locked is terminal for local transitions.',
    source: 'src-tauri/src/licensing/state_machine.rs::is_legal_transition',
    notes: [
      'ENFORCED graph only. Doc 05 §10 also shows an aspirational graph (Trial/Active/Grace/PastDue/Canceled/Locked) that no code enforces.',
      'Locked has no outgoing edges — escaping requires license_activate with fresh server-verified state (a different write path that deliberately bypasses this graph).',
      'PastDue, Canceled, Expired parse from the server but participate in NO local transition.',
      'transition(conn, same_status) returns Ok(()) without writing — a no-op is not an error.',
    ],
    states: [
      { id: 'start', label: 'Start', start: true },
      {
        id: 'AnonymousEval',
        label: 'AnonymousEval',
        note: 'Starting state for a never-activated install',
      },
      { id: 'Trial', label: 'Trial', note: '14-day window derived from local_profile.created_at' },
      { id: 'Active', label: 'Active' },
      { id: 'Grace', label: 'Grace', note: '7-day offline grace from last_server_validated_at' },
      { id: 'Locked', label: 'Locked', terminal: true, note: 'Terminal for local transitions' },
    ],
    transitions: [
      {
        from: 'start',
        to: 'AnonymousEval',
        label: 'fresh install',
        detail: 'Preserves clock-skew locking on fresh install',
      },
      { from: 'AnonymousEval', to: 'Trial', label: 'trial derived' },
      { from: 'Trial', to: 'Active', label: 'payment confirmed' },
      { from: 'Active', to: 'Grace', label: 'backend unreachable' },
      { from: 'Grace', to: 'Active', label: 'reconnected' },
      { from: 'Grace', to: 'Locked', label: 'grace exhausted (7d)' },
      { from: 'Active', to: 'Locked', label: 'clock skew > 1h / revoked' },
      { from: 'Trial', to: 'Locked', label: 'trial expired' },
      { from: 'AnonymousEval', to: 'Locked', label: 'clock skew' },
    ],
  },
  {
    id: 'statement',
    name: 'Statement Lifecycle',
    summary:
      'A parked statement never blocks the pipeline — awaiting_password is non-blocking by design.',
    source: 'docs/design/03 §10 + src-tauri/src/statements/',
    notes: [
      'awaiting_password NEVER blocks: resolve_statement_password writes the row, stores the encrypted PDF, emits statement_password_required and returns PromptCreated — the caller moves to the next file.',
      'Re-entry points: statements_retry_unprocessed and statements_reparse_all, possible because the PDF bytes were persisted (encrypted) at intake.',
      'Wrong password re-prompts without losing context — the unprocessed row is not deleted.',
      'Composite view across three status columns: statements.parse_status (queued → processing → parsed | failed), unprocessed_statements.status (awaiting_password, awaiting_instrument_confirmation) and statement_drafts.status (pending_review, committed, discarded).',
    ],
    states: [
      { id: 'start', label: 'Start', start: true },
      { id: 'queued', label: 'queued' },
      { id: 'awaiting_password', label: 'awaiting_password', note: 'parked, non-blocking' },
      {
        id: 'awaiting_instrument_confirmation',
        label: 'awaiting_instrument_confirmation',
        note: 'parked, non-blocking',
      },
      { id: 'pending_review', label: 'pending_review', note: 'user-editable draft' },
      { id: 'committed', label: 'committed', terminal: true },
      { id: 'discarded', label: 'discarded', terminal: true },
    ],
    transitions: [
      { from: 'start', to: 'queued', label: 'insert_queued (crash-recovery invariant)' },
      {
        from: 'queued',
        to: 'awaiting_password',
        label: 'no working password',
        detail: 'Pipeline continues; never blocks',
      },
      {
        from: 'queued',
        to: 'awaiting_instrument_confirmation',
        label: 'Instrument Gate blocks',
        detail: 'Issuer/masked-ID missing — never defaulted',
      },
      { from: 'queued', to: 'pending_review', label: 'staging succeeds' },
      { from: 'awaiting_password', to: 'pending_review', label: 'password supplied or re-parse' },
      {
        from: 'awaiting_instrument_confirmation',
        to: 'pending_review',
        label: 'user confirms instrument',
      },
      { from: 'pending_review', to: 'committed', label: 'commit_staged_draft' },
      { from: 'pending_review', to: 'discarded', label: 'statements_discard_draft' },
    ],
  },
  {
    id: 'scan',
    name: 'Historical Scan Checkpoint',
    summary:
      'Resumable scan state — only one in_progress scan per account, enforced by atomic claim.',
    source: 'src-tauri/src/db/processing_checkpoints.rs + ingestion/historical_scan.rs',
    notes: [
      'The `paused` status is checked by resume logic but NO code path sets it — pause pauses the in-memory loop without changing persisted status.',
      'Crash mid-scan leaves status="in_progress" forever until re-claimed; there is no stale-lock detection.',
    ],
    states: [
      { id: 'start', label: 'Start', start: true },
      { id: 'not_started', label: 'not_started' },
      { id: 'in_progress', label: 'in_progress' },
      { id: 'completed', label: 'completed' },
      { id: 'cancelled', label: 'cancelled' },
      { id: 'failed', label: 'failed' },
    ],
    transitions: [
      { from: 'start', to: 'not_started', label: 'no checkpoint' },
      { from: 'not_started', to: 'in_progress', label: 'claim checkpoint (atomic)' },
      { from: 'in_progress', to: 'completed', label: 'all messages processed' },
      { from: 'in_progress', to: 'cancelled', label: 'scans_cancel' },
      { from: 'in_progress', to: 'failed', label: 'search / processing error' },
      { from: 'failed', to: 'in_progress', label: 'scans_resume' },
      { from: 'cancelled', to: 'in_progress', label: 're-claim' },
      { from: 'completed', to: 'in_progress', label: 'new date range' },
    ],
  },
  {
    id: 'account',
    name: 'Connected Account Lifecycle',
    summary: 'Gmail account status: ACTIVE / degraded / disconnected.',
    source: 'src-tauri/src/ingestion/oauth.rs + polling.rs',
    notes: [
      'History-too-old (404/400) does NOT change account status — it only clears the cursor. The account stays ACTIVE but is skipped every cycle until a fresh historical scan re-seeds the cursor (no auto-heal).',
    ],
    states: [
      { id: 'start', label: 'Start', start: true },
      { id: 'ACTIVE', label: 'ACTIVE' },
      { id: 'degraded', label: 'degraded' },
      { id: 'disconnected', label: 'disconnected', terminal: true },
    ],
    transitions: [
      { from: 'start', to: 'ACTIVE', label: 'OAuth connect succeeds' },
      {
        from: 'ACTIVE',
        to: 'degraded',
        label: 'token refresh fails',
        detail: 'mark_account_degraded_async',
      },
      { from: 'degraded', to: 'ACTIVE', label: 'user reconnects' },
      { from: 'ACTIVE', to: 'disconnected', label: 'user disconnects' },
    ],
  },
  {
    id: 'onboarding',
    name: 'Onboarding Phases',
    summary:
      'Un-skippable where required; consent is recorded before any network feature is reachable.',
    source: 'src/routes/onboarding/OnboardingFlow.tsx + pages/Onboarding.tsx',
    notes: [
      '`dinero_onboarded` is set at the gmail_consent → historical_scan transition (or on Gmail skip), NOT at the final step — a user who quits before step 4 lands on Dashboard next launch.',
      'Phase state is purely client-side useState; a refresh restarts at welcome.',
    ],
    states: [
      { id: 'start', label: 'Start', start: true },
      { id: 'welcome', label: 'welcome' },
      { id: 'disclosure', label: 'disclosure', note: 'network disclosure consent' },
      { id: 'preferences', label: 'preferences' },
      { id: 'gmail_consent', label: 'gmail_consent' },
      { id: 'historical_scan', label: 'historical_scan' },
      { id: 'license_confirm', label: 'license_confirm' },
      { id: 'onboarded', label: 'onboarded', terminal: true },
    ],
    transitions: [
      { from: 'start', to: 'welcome', label: 'not onboarded' },
      { from: 'welcome', to: 'disclosure', label: 'Get Started' },
      { from: 'disclosure', to: 'preferences', label: 'Continue (consent recorded)' },
      { from: 'preferences', to: 'gmail_consent', label: 'statement pref = auto' },
      { from: 'preferences', to: 'license_confirm', label: 'statement pref = manual (skip Gmail)' },
      { from: 'gmail_consent', to: 'historical_scan', label: 'OAuth success' },
      { from: 'gmail_consent', to: 'license_confirm', label: 'Skip (manual only)' },
      { from: 'historical_scan', to: 'license_confirm', label: 'Continue' },
      { from: 'license_confirm', to: 'onboarded', label: 'Continue to Dashboard' },
    ],
  },
];
