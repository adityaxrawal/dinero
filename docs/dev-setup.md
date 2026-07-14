# Dinero — Local Development Setup

TASK-SETUP-015. This is the detailed companion to the README's quick-start section.

## Prerequisites

- **macOS ≥ 13 (Ventura)** — the app targets macOS only; SQLCipher's `bundled-sqlcipher` build and the local Candle LLM runtime both assume a macOS SDK.
- **Rust**: `rustup update stable` — keep the toolchain current; CI builds against `stable`.
- **Node.js ≥ 20** — required by the Vite/React toolchain and the CI pin (`.github/workflows/react.yml`).
- **pnpm** (version 9, matching CI) — `corepack enable` or `npm install -g pnpm@9`.
- **Xcode Command Line Tools** — `xcode-select --install`. Required for the macOS SDK headers `rusqlite`'s `bundled-sqlcipher` feature needs to compile, and for code signing later (TASK-DESK-006).

## Install and Run

```bash
pnpm install
pnpm tauri dev
```

This starts the Vite dev server and the Tauri Rust backend together, opening the app window. First run will trigger a **macOS Keychain permission prompt** — this is expected, not an error. Dinero stores the SQLCipher database key, Gmail OAuth tokens, and session tokens exclusively in Keychain (Document 15 §10); denying the prompt will prevent the app from starting (fail-closed by design, Document 15 Core Principle 12).

## Google OAuth Development Credentials

Gmail ingestion (TASK-AUTH-001) uses Google's PKCE flow — no client secret is required, but a **Client ID** is:

1. In [Google Cloud Console](https://console.cloud.google.com/), create (or reuse) a project and configure an OAuth consent screen.
2. Create an OAuth Client ID of type **Desktop app**.
3. `GOOGLE_CLIENT_ID` is baked in at **compile time** via `option_env!("GOOGLE_CLIENT_ID")` (`src-tauri/src/ingestion/oauth.rs`) — export it before building/running:
   ```bash
   export GOOGLE_CLIENT_ID="your-client-id.apps.googleusercontent.com"
   pnpm tauri dev
   ```
   Without it, `GOOGLE_CLIENT_ID` compiles to an empty string and the OAuth flow will fail at runtime — Gmail-dependent features simply won't work locally, everything else (statement upload, manual transactions, dashboard) is unaffected.

## Resetting the Local Database

To start fresh (new schema, corrupted dev DB, testing onboarding flow again):

```bash
rm -rf ~/Library/Application\ Support/com.dinero.app/finance.db*
```

This also removes the SQLCipher backup files (`finance.db.bak.*`). The Keychain-stored encryption key is unaffected by this — if you want a fully clean slate including Keychain state, also remove the `com.dinero.app` service entries via Keychain Access.app.

## Privacy Invariants — What Must Never Reach the Network

These hold in every build, dev included (Document 15 §2 Core Principles, Document 01 §10.4):

- **No financial data leaves the device, ever** — no transaction, statement, instrument, or balance data is sent to any network destination, in any build configuration.
- **Only five network destinations exist, system-wide**, all called from the Rust backend only, never the React WebView: Gmail API (read-only polling), the Licensing Backend, Google OAuth servers (PKCE handshake), GitHub Releases (version check), and the one-time Hugging Face local-LLM model download. None of the five carry financial data.
- **No sixth channel** — don't add a new `fetch`/`XMLHttpRequest`/network call from React under any circumstance (enforced by the CSP in `src-tauri/tauri.conf.json`, TASK-SETUP-002); new Rust-side network calls must be one of the five above or require a documented architecture decision (Document 48).
- **Raw PDFs are never persisted to disk**, in dev or production — parsed in memory only, regardless of whether they arrive via Gmail attachment or manual upload.
- **Secrets never go to logs, SQLite, or plaintext config** — Gmail OAuth tokens, the SQLite encryption key, and session tokens live in macOS Keychain only (the Licensing JWT is the sole intentional exception, stored in `license_state.license_jwt` — its guarantee is signature unforgeability, not secrecy).

## Formatting & Linting

```bash
pnpm lint       # ESLint
pnpm typecheck  # tsc --noEmit
pnpm format     # Prettier --write

cd src-tauri
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```
