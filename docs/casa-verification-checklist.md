# Google CASA Tier 3 Verification Readiness Checklist

**Purpose (Document 21 §2.3):** Dinero's Google OAuth client is currently in
Google's "Testing" publishing status, capped at 100 test users
(`BETA_PROGRAM_DISCLOSURE` in `src/constants/privacy.ts`, surfaced on the
onboarding consent screen). Before `gmail.readonly` can be used beyond that
cap, the app must pass Google's CASA (Cloud Application Security Assessment)
Tier 3 review. This checklist tracks readiness against CASA's actual
requirements, verified against this codebase's current state — not assumed.

**Contingency (Document 30 TASK-AUTH-016):** if CASA fails or is delayed past
the 100-user cap, Statement-Only Mode disables Gmail features gracefully
rather than crashing. See the "Statement-Only Mode" section below for its
actual current status — it is only partially built as of this checklist.

---

## 1. Token storage — Keychain only, never SQLite

**Status: Verified.**

- `ingestion::oauth::save_token`/`get_token` store and read OAuth tokens
  exclusively via `keyring::Entry` (macOS Keychain), under service
  `com.dinero.app`, one entry per connected account
  (`keychain_account_name`).
- `ConnectedAccountsRow` (the struct actually written to the `connected_accounts`
  SQLite table) has no token field at all — verified as a compile-time
  guarantee, not just a runtime check, by
  `secrets_audit.rs::test_gmail_tokens_stored_only_in_keychain` (TASK-AUTH-007):
  the struct literal there is exhaustive, so a future field added to
  `ConnectedAccountsRow` without updating that test is a compile error.
- The same test scans every table's actual columns (`PRAGMA table_info`) for
  any token/secret-shaped column name across the whole schema, not just
  `connected_accounts`.

## 2. Consent screen matches requested scopes exactly — no scope creep

**Status: Verified.**

- Exactly one scope is ever requested:
  `https://www.googleapis.com/auth/gmail.readonly`
  (`ingestion::oauth::start_oauth_flow_async`, one `add_scope` call). No
  `openid`/`email`/`profile` scope is requested — the connected account's
  email address is obtained post-token via Gmail API's own
  `users.getProfile` endpoint, which `gmail.readonly` alone grants
  (`GmailClient::get_profile`).
- The onboarding consent screen (`src/pages/Onboarding.tsx`, step 3) renders
  this exact scope string verbatim under "Requested Scopes" before the user
  can proceed, alongside the full outbound-channels disclosure
  (`OUTBOUND_CHANNEL_DISCLOSURE`) and the beta-program limitations
  (`BETA_PROGRAM_DISCLOSURE`) — both single sources of truth also rendered
  in Settings → Privacy and (per their own doc comments) intended for the
  Privacy Policy.
- The system browser is used for the OAuth flow (`tauri_plugin_opener::open_url`),
  never an embedded webview — required by CASA and already Document 22
  §5.1's stated architecture.

## 3. Security questionnaire preparation

Facts to draw from when CASA's questionnaire is actually issued (this
section summarizes current architecture; it is not itself the submitted
questionnaire):

- **Local-only architecture:** all financial data (transactions, statements,
  instruments) lives exclusively in a local, SQLCipher-encrypted SQLite
  database (`finance.db`). The only network destinations are the five
  channels disclosed in `OUTBOUND_CHANNEL_DISCLOSURE` (Gmail API, Licensing
  Backend, Google OAuth, GitHub Releases, Hugging Face) — verified by the
  Network Activity Log (`network_client.rs` routes every outbound call
  through one logging point) and `secrets_audit.rs`'s schema-wide scan.
- **Encryption at rest:** SQLCipher (AES-256), key derived via Argon2id from
  a Keychain-stored base key plus the Mac's hardware UUID
  (`db::crypto::derive_database_key`) — never written to disk itself
  (`secrets_audit.rs::test_sqlite_key_never_written_to_disk` scans every
  file actually written during a real `init_db` for the derived key
  appearing anywhere in raw bytes).
- **CSP:** `tauri.conf.json`'s `csp` is `default-src 'self'; script-src
  'self'; connect-src 'self' ipc: http://ipc.localhost
  https://license.dinero.app; img-src 'self' data:; style-src 'self'
  'unsafe-inline'` — the WebView cannot reach the Gmail API or Google OAuth
  domains directly at all; those calls are made from the Rust backend only,
  never from renderer JS.
- **No third-party analytics or crash reporting** leaves the device
  (`OUTBOUND_CHANNEL_DISCLOSURE`'s explicit last line); crash reports are
  captured locally, encrypted, and only leave the device if the user
  manually exports and sends a diagnostic bundle (TASK-AUTH-015), which
  itself is scanned for PII before the ZIP is even written
  (`diagnostics::scan_for_pii`).

## 4. Statement-Only Mode contingency

**Status: Partially built.** A full graceful-degradation mode (an existing
Gmail-connected user losing Gmail access mid-use, e.g. if CASA verification
lapses post-launch, without the app crashing or losing statement-upload
functionality) is Document 30 TASK-FE-017's job — that task has not been
reached yet (Area 9, frontend).

What already exists: the onboarding flow (`Onboarding.tsx`) lets a user skip
Gmail entirely at first launch (`statementPref: 'manual'`), landing on a
fully functional manual-statement-upload setup with no Gmail dependency.
This covers the *new-user* path but not the *already-connected user loses
Gmail access* path TASK-FE-017 is meant to cover — that gap is real and
should be tracked before relying on this contingency in production.

## 5. Scheduling

Complete this review **before** the 100-user Testing-mode cap is reached —
`assert_new_gmail_account_allowed` (`ingestion::oauth.rs`) enforces a
license-gated cap on *this app's own* concurrently-connected-account limit
(10 per Document 03 §8.2), which is a separate, unrelated limit from
Google's 100-*total-test-user* cap on the OAuth client itself; the latter
has no enforcement mechanism in this codebase (it's a Google Cloud Console
setting, external to this repo) and must be tracked operationally.
