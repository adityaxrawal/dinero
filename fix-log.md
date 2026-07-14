# Fix Log

Append-only. One entry per task processed under the §3 verification protocol (Master Execution Prompt).

---

## TASK-SETUP-001: Initialize Tauri 2.x Project with React/Vite Template

**Found:** Repo already has a fully scaffolded Tauri v2 + React/Vite project, well beyond initial-blank-window state (src-tauri has ~122 Rust files across db/commands/ipc/licensing/extraction/ingestion/reconciliation/statements; src/ has 42 TS/TSX files). `identifier` was already correctly `com.dinero.app`. `bundle.targets` already `["app", "dmg"]` (macOS-only, matches spec).

**Deviation found:** `productName` was `"dinero-app"`; Document 30 TASK-SETUP-001 explicitly specifies `"productName": "Dinero"`.

**What I did:** Corrected `productName` to `"Dinero"` in `src-tauri/tauri.conf.json`. Left everything else in the file untouched — CSP (`security.csp`) and window dimensions are TASK-SETUP-002/004's scope respectively, not this task's; noted for later (CSP currently allows direct WebView `connect-src` to Google/GitHub domains, which looks like it will conflict with Document 22 §18.1's "those calls are Rust-only, never WebView" — will re-verify when I reach TASK-SETUP-002).

**Verified:** `cargo check` in `src-tauri/` compiles clean (0 errors). `pnpm typecheck` surfaces pre-existing unused-import errors in `src/pages/Instruments.tsx` — unrelated to this task's scope (TASK-SETUP-001 has no acceptance criteria beyond the initial scaffold boot, which is long since superseded by the app's current state); not fixed here to avoid scope creep into TASK-SETUP-007/019.

---

## TASK-SETUP-002: Configure Strict Content Security Policy

**Found:** `src-tauri/tauri.conf.json`'s `security.csp` deviated from Document 30's spec in two concrete ways: (1) `connect-src` let the WebView reach `accounts.google.com`, `oauth2.googleapis.com`, `www.googleapis.com`, `github.com`, and `objects.githubusercontent.com` directly — contradicting Document 22 §18.1's requirement that Google/GitHub calls happen exclusively from the Rust backend, never the WebView (Document 15 Core Principle 3, Core Principle 4); (2) the licensing-backend origin was `https://api.dinero-app.com` instead of the spec's `https://license.dinero.app`. Confirmed via `grep` that no frontend code (`src/`) makes any `fetch`/`XMLHttpRequest`/`axios` call at all, and the one `googleapis.com` string match in `Onboarding.tsx` is disclosure copy in JSX text, not a network call — so removing these origins breaks nothing.

**What I did:** Rewrote `security.csp` to match Document 30's exact string: `default-src 'self'; script-src 'self'; connect-src 'self' ipc: http://ipc.localhost https://license.dinero.app; img-src 'self' data:; style-src 'self' 'unsafe-inline'`. Dropped the unused `asset:` img-src scheme and the standalone `font-src 'self'` directive (no code references either — `default-src 'self'` already covers font fallback under CSP inheritance rules). Kept Tauri's own `ipc:`/`http://ipc.localhost` connect-src entries: these are Tauri v2's own internal IPC transport (not third-party network egress) and removing them would break `invoke()` itself — this is the standard Tauri v2 scaffold CSP entry, a framework requirement, not a deviation from Document 22 §18.1's intent (which is about third-party destinations, not the local Rust↔WebView bridge Document 15 Core Principle 3 mandates as the *only* channel).

**Flagged, not silently resolved:** the task's acceptance criterion ("CI check: production build CSP contains no `localhost` reference") is written assuming only the dev-mode `ws://localhost:*` allowance the task text describes. Read completely literally it would also flag Tauri's own required `http://ipc.localhost` virtual host, which has nothing to do with the dev-server concern the criterion is guarding against and is present in every Tauri v2 scaffold. Treating this as a resolvable engineering-convention judgment call (Document 49 §4 precedent), not a product/business tradeoff worth stopping for. The CI check itself (TASK-SETUP-010) should be written to check for the dev-only `ws://localhost:*` pattern specifically, not a bare `localhost` substring match.

**Verified:** `python3 -c "import json; json.load(open('src-tauri/tauri.conf.json'))"` confirms valid JSON. No frontend code depends on the removed origins (grep above).

---

## TASK-SETUP-003: Configure Tauri IPC Allowlists (Disable Generic Shell/FS Access)

**Found:** Document 30's task text uses Tauri v1 terminology (`tauri.allowlist.shell/fs/http` with `"all": false`) which no longer exists in Tauri v2 — v2 replaced the allowlist model with an opt-in capabilities/permissions system where nothing is exposed to the WebView unless a plugin is added as a Cargo dependency *and* granted a permission in `capabilities`. Checked `src-tauri/Cargo.toml`: no `tauri-plugin-fs`, `tauri-plugin-shell`, or `tauri-plugin-http` dependency exists. Checked `tauri.conf.json`'s inline capability (`src-tauri/capabilities/` directory itself is empty — permissions are declared inline): only `core:default`, `opener:default`, `dialog:default` are granted. Checked all `fs::`/`std::fs` usage in `src-tauri/src` — every call site is Rust-native (`std::fs`, `tokio::fs`) inside backend modules, never exposed to the frontend. The `opener:default` permission is a deliberate, scoped exception for `tauri_plugin_opener::open_url()` (used once, in `src-tauri/src/ingestion/oauth.rs`, to launch the system browser for Google OAuth per TASK-AUTH-001) — not a generic shell-open capability.

**What I did:** No code change. This is the Tauri-v2-equivalent of "disabled" already, structurally: the WebView has no path to raw filesystem, shell execution, or generic HTTP that doesn't already run through a typed `invoke()` command. Confirmed match, no quality gaps found.

**Verified:** `grep` confirms no fs/shell/http plugin dependencies and no matching permission strings anywhere in `tauri.conf.json` or `src-tauri/capabilities/`.

---

## TASK-SETUP-004: Configure Window Dimensions and Startup Behavior

**Found:** `tauri.conf.json`'s `app.windows[0]` deviated from Document 30's exact spec on four fields: `title` was `"dinero-app"` (spec: `"Dinero"`), `width` was `1200` (spec: `1280`), `minWidth` was `800` (spec: `900`), and `transparent` was `true` (spec: `false` — the doc explicitly calls out non-transparent as a deliberate choice to avoid alpha-compositing overhead on older Macs). `resizable`, `decorations`, and `center` were absent (relying on Tauri defaults, which happen to match `resizable: true`/`decorations: true`, but `center` defaults to `false`, not `true`).

**What I did:** Corrected all four deviating fields and added explicit `resizable: true`, `decorations: true`, `center: true` so the config states its intent rather than relying on implicit defaults.

**Verified:** `python3 -c "import json; json.load(...)"` confirms valid JSON. No acceptance criteria beyond manual launch verification (per doc); config now matches spec exactly.
