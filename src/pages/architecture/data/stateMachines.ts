/**
 * State machines verified against source (2026-08-01; license, scan and
 * account re-verified 2026-11-23 after the system design review fixes).
 *
 * The license machine is the one that matters most for correctness. It has two
 * write paths: local edges (is_legal_transition, checked by model.test.ts) and
 * server-verified updates (marked `server: true`). The scan machine's states
 * are also checked against the statuses historical_scan.rs uses.
 */
import type { StateMachineDef } from './types';

export const STATE_MACHINES: StateMachineDef[] = [
  {
    id: 'license',
    name: 'License State Machine',
    summary:
      'Eight statuses. Local, time-driven moves follow 8 fixed edges; a verified server answer can set any server state, including lifting Locked.',
    source:
      'src-tauri/src/licensing/state.rs + state_machine.rs (is_legal_transition, server_status, apply_server_validation) + gate.rs',
    notes: [
      'Two write paths. (1) Local transitions (offline grace, clock skew, trial expiry) go through is_legal_transition and its 8 edges. (2) A server validation whose RS256 JWT verifies goes through apply_server_validation, which may set any server state: the server is authoritative.',
      'Locked is NOT terminal. A renewal is picked up by the next background validation (6h) or a manual Refresh and moves Locked → Active; license_activate also works. A local edge out of Locked still does not exist.',
      'Server states: TRIAL, ACTIVE, PAST_DUE, LOCKED. The backend signs the state into the JWT `state` claim; the client trusts the claim over the response body and treats anything unrecognised (or missing) as LOCKED. Cancelled, halted, completed and expired subscriptions, and a trial past its end date, are all signed LOCKED.',
      'Write gate: only Active, Grace, PastDue and an unexpired Trial may write. Locked, Canceled, Expired and AnonymousEval are read-only. A lapsed licence also stops background Gmail polling.',
      'Canceled and Expired still exist in the enum (older rows parse into them) but the current server mapping never produces them; the gate refuses both.',
      'The trial is anchored on the server: with no license_state row, the worker calls start-trial by device id at first launch and stores the signed TRIAL token. A data reset gets the same trial back. Only an unregistered (offline first launch) trial falls back to the local 14-day window.',
      "Tokens last 7 days, matching the offline grace. validate and deactivate require the device's current JWT as proof.",
      'transition(conn, same_status) returns Ok(()) without writing — a no-op is not an error.',
    ],
    states: [
      { id: 'start', label: 'Start', start: true },
      {
        id: 'AnonymousEval',
        label: 'AnonymousEval',
        note: 'No licence yet, or deactivated. Read-only',
      },
      { id: 'Trial', label: 'Trial', note: 'Server-registered 14-day trial (TRIAL)' },
      { id: 'Active', label: 'Active', note: 'Paid and validated (ACTIVE)' },
      { id: 'Grace', label: 'Grace', note: 'Backend unreachable; up to 7 days offline' },
      { id: 'PastDue', label: 'PastDue', note: 'Renewal charge failed; still entitled (PAST_DUE)' },
      { id: 'Locked', label: 'Locked', note: 'Read-only until a server validation lifts it' },
    ],
    transitions: [
      { from: 'start', to: 'AnonymousEval', label: 'fresh install' },
      { from: 'AnonymousEval', to: 'Trial', label: 'trial registered' },
      { from: 'Trial', to: 'Active', label: 'payment confirmed' },
      { from: 'Active', to: 'Grace', label: 'backend unreachable' },
      { from: 'Grace', to: 'Active', label: 'reconnected' },
      { from: 'Grace', to: 'Locked', label: 'grace exhausted (7d)' },
      { from: 'Active', to: 'Locked', label: 'clock skew > 1h' },
      { from: 'Trial', to: 'Locked', label: 'trial expired' },
      { from: 'AnonymousEval', to: 'Locked', label: 'clock skew' },
      { from: 'Active', to: 'PastDue', label: 'renewal failed (server)', server: true },
      { from: 'PastDue', to: 'Active', label: 'renewal paid (server)', server: true },
      { from: 'PastDue', to: 'Locked', label: 'subscription ended (server)', server: true },
      { from: 'Active', to: 'Locked', label: 'cancelled / refunded (server)', server: true },
      {
        from: 'Locked',
        to: 'Active',
        label: 'renewed (server validation)',
        detail: 'apply_server_validation; not a local edge',
        server: true,
      },
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
      'Resumable scan state per account — one in_progress scan at a time (atomic claim). Cancelling deletes the checkpoint.',
    source: 'src-tauri/src/ingestion/historical_scan.rs + db/processing_checkpoints.rs + db/mod.rs',
    notes: [
      'not_started is not stored: scans_status reports it when no checkpoint row exists.',
      'There is no cancelled status. Cancel rolls back the rows the scan created (only those created since it started) and deletes the checkpoint, so the account is back to not_started.',
      'paused is written only by scans_retry_failed, to seed a run over the messages in scan_failed_messages. Pausing the scan queue (debug) pauses the loop in memory and does not change the stored status.',
      'completed_with_errors: after the main pass, failed messages get one automatic retry pass (60s pause); anything still failing stays in scan_failed_messages and the scan ends completed_with_errors. Settings → Mail Scan offers "Retry failed (N)".',
      'On startup every in_progress checkpoint is rewritten to failed (crash recovery), so a crashed scan never holds the claim.',
      'Resume continues from resume_from, the contiguous finished prefix — not processed_count, since fetches finish out of order. A saved run is resumed only when the date range matches; a different range starts fresh.',
    ],
    states: [
      { id: 'start', label: 'Start', start: true },
      { id: 'not_started', label: 'not_started', note: 'no checkpoint row' },
      { id: 'in_progress', label: 'in_progress' },
      { id: 'paused', label: 'paused', note: 'seeded by Retry failed' },
      { id: 'completed', label: 'completed' },
      { id: 'completed_with_errors', label: 'completed_with_errors' },
      { id: 'failed', label: 'failed' },
    ],
    transitions: [
      { from: 'start', to: 'not_started', label: 'no checkpoint' },
      { from: 'not_started', to: 'in_progress', label: 'claim checkpoint (atomic)' },
      { from: 'in_progress', to: 'completed', label: 'all messages processed' },
      {
        from: 'in_progress',
        to: 'completed_with_errors',
        label: 'failures left after the retry pass',
      },
      { from: 'in_progress', to: 'not_started', label: 'cancel (checkpoint deleted)' },
      { from: 'in_progress', to: 'failed', label: 'scan error / app restart' },
      { from: 'failed', to: 'in_progress', label: 'scans_resume' },
      { from: 'completed', to: 'in_progress', label: 'new scan' },
      { from: 'completed_with_errors', to: 'paused', label: 'Retry failed' },
      { from: 'completed_with_errors', to: 'in_progress', label: 'new scan' },
      { from: 'paused', to: 'in_progress', label: 'claim (retry run)' },
    ],
  },
  {
    id: 'account',
    name: 'Connected Account Lifecycle',
    summary:
      'Gmail account status: ACTIVE / degraded / disconnected. degraded means only a reconnect can fix it.',
    source: 'src-tauri/src/ingestion/oauth.rs + polling.rs + health.rs',
    notes: [
      'degraded is reserved for permanent auth failures: invalid_grant, invalid_client, unauthorized_client, a missing or unparseable token, or no refresh token. The poller skips degraded accounts.',
      'Transient failures (network errors, timeouts, unknown errors, a locked or denied Keychain) keep the account ACTIVE: they are audited (gmail_token_refresh_transient_failure) and the next poll retries.',
      'The stored token is deleted only when Google reports the grant invalid (invalid_grant / invalid_client).',
      "History-too-old (404/400) does not change status: the poller re-seeds the cursor from Gmail's current historyId and starts a catch-up scan from a day before the cursor was last good.",
      'Visible, not silent: the 60s health loop raises gmail_token_degraded (with a reconnect hint) while any account is degraded, and gmail_sync_stale when an ACTIVE account has had no successful poll for 15 minutes. Both clear on recovery.',
      'Repeated permanent failures (3 in 1 hour, per account) are audited as a security incident but no longer revoke other accounts or log the user out.',
      'Status changes emit connected_accounts_changed, which the UI listens for.',
    ],
    states: [
      { id: 'start', label: 'Start', start: true },
      { id: 'ACTIVE', label: 'ACTIVE' },
      { id: 'degraded', label: 'degraded', note: 'needs reconnect' },
      { id: 'disconnected', label: 'disconnected', terminal: true },
    ],
    transitions: [
      { from: 'start', to: 'ACTIVE', label: 'OAuth connect succeeds' },
      {
        from: 'ACTIVE',
        to: 'degraded',
        label: 'permanent auth failure',
        detail: 'record_refresh_failure → mark_account_degraded_async',
      },
      {
        from: 'ACTIVE',
        to: 'ACTIVE',
        label: 'transient failure / history 404',
        detail: 'retry next poll; 404 re-seeds cursor + catch-up scan',
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
