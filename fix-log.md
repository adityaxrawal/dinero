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

---

## TASK-SETUP-005: Add All Rust Cargo Dependencies

**Found:** Nearly every dependency Document 30 lists is already present in `src-tauri/Cargo.toml` (tauri, tokio full, rusqlite bundled-sqlcipher, deadpool-sqlite, serde/serde_json, reqwest json/rustls-tls, keyring, thiserror/anyhow, tracing/tracing-subscriber, uuid v4, sha2, argon2, aes-gcm, regex, chrono, strsim, sysinfo, oauth2). Two nominal gaps against the doc's literal list:
1. **`once_cell`** — absent, but every place that would use it (`ladder.rs`, `message_processor.rs`) already uses `std::sync::OnceLock`, the stdlib equivalent stabilized after `once_cell` became the de facto standard this crate historically filled. Adding an unused `once_cell` dependency alongside an already-idiomatic stdlib solution would be pure noise — not adding it.
2. **`sqlx`** — genuinely absent. Document 30's own TASK-DB-002 explicitly specifies `sqlx::migrate!` as the migration runner, but the existing, already-built migration system (`src-tauri/src/db/migrations.rs`) uses the `rusqlite_migration` crate instead — a real architectural substitution, not a missing-dependency oversight. This is a substantive Area 2 decision (keep `rusqlite_migration` and treat Document 30 as superseded-in-practice, or migrate the already-built system to `sqlx`), not something to resolve by silently adding an unused dependency inside a manifest-listing task. **Flagged for TASK-DB-001/002**, not resolved here.

**What I did:** No `Cargo.toml` change. `cargo build`/`cargo check` already succeed (verified under TASK-SETUP-001), which is this task's only stated acceptance criterion.

**Verified:** `cargo check` clean (see TASK-SETUP-001/002 verification runs, dependency set unchanged since).

---

## TASK-SETUP-006: Add macOS RAM Check on App Startup

**Found:** No RAM-gating logic existed in the Tauri `setup()` hook at all. `check_system_ram` (in `src-tauri/src/commands/mod.rs`) is an on-demand IPC command returning RAM in GB — unrelated to this task, which requires an automatic startup check that sets an app-wide `llm_eligible` state and emits a warning event. `llm_manager.rs` has the 5-tier model catalog (`min_ram_gb` per tier, matching Document 16 §12.3) but nothing reads actual system RAM against it at launch. The `AppEvent::SystemWarning` enum variant (`"system_warning"`) already existed in `src-tauri/src/ipc/events.rs` but was never emitted anywhere in the codebase.

**Naming note:** Document 30's task text says emit `system.warning` with fields `{ type, available_gb }`; Document 19 §15.1 (the authoritative Tauri Events catalog) names the event `system_warning` with fields `warning_type`/`message`. Followed Document 19 since it is the authoritative source for all Tauri event names and is explicitly self-described as such — added `available_gb` as a third payload field since Document 19 doesn't prohibit additive fields (§19 versioning policy: additive changes are fine).

**What I did:** Implemented from scratch — new module `src-tauri/src/startup.rs`: `compute_llm_eligibility(ram_gb)` (pure, unit-testable), and `check_ram_and_set_llm_eligibility(app)` which reads RAM via `sysinfo::System::new_all().refresh_memory()`, calls `app.manage()` with an `LlmEligibility { eligible, total_ram_gb }` state (eligible iff RAM ≥ 16 GB, matching Document 16 §12.3's auto-eligible tier — the 8–16 GB tier's smaller models remain available only via a manual settings override that TASK-TXN-006 will wire in when it actually consumes this state), and emits `system_warning` if RAM < 8 GB. Wired into `lib.rs`'s `setup()` hook, running before DB init (which can itself exit the process on several error paths) so the RAM check is never skipped. The function is synchronous, infallible, and never panics or blocks.

**Verified:** `cargo check` clean. 5 new unit tests in `startup.rs` (`low_ram_is_not_llm_eligible`, `mid_tier_8_to_16gb_is_not_auto_eligible`, `sixteen_gb_is_eligible`, `high_ram_is_eligible`, `boundary_just_below_sixteen_is_not_eligible`) — all pass (`cargo test --lib startup::`).

---

## TASK-SETUP-007: Configure TypeScript Strict Mode and Path Aliases

**Found:** `tsconfig.json` already had `strict`, `noUnusedLocals`, `noUnusedParameters` — but `target` was `ES2020` (spec: `ES2022`), `exactOptionalPropertyTypes` was absent, and only one path alias (`@/*`) existed against the spec's five (`@/*`, `@components/*`, `@hooks/*`, `@types/*`, `@stores/*`). `vite.config.ts` only mirrored the single `@` alias, so the other four wouldn't have resolved at build/runtime even if added to `tsconfig.json` alone. Also confirmed (from TASK-SETUP-001's note) that `npm run tsc` did **not** pass cleanly — this task's own acceptance criterion — due to unused imports in `Instruments.tsx`.

**What I did:** Bumped `target`/`lib` to `ES2022`, added `exactOptionalPropertyTypes: true` and the four missing path aliases to `tsconfig.json`, and mirrored all five in `vite.config.ts`'s `resolve.alias` (`@types`/`@stores` point at directories that don't exist yet — harmless for now, needed once TASK-FE-002's Zustand store and any dedicated types module land). Fixed the pre-existing `Instruments.tsx` unused-import errors (`CardHeader`, `CardTitle`, `CardDescription` — confirmed unused via grep). `exactOptionalPropertyTypes` then surfaced one genuine, previously-latent type error in `src/hooks/use-toast.ts`: `dismiss()`'s `dispatch({ type: "DISMISS_TOAST", toastId })` call passes a possibly-`undefined` value into a field typed as bare-optional (`toastId?: string`), which the new flag correctly rejects as different from "field absent." Fixed by widening `DISMISS_TOAST`/`REMOVE_TOAST`'s `toastId` to `?: string | undefined` (matching the TS error's own suggested fix) rather than changing the dispatch call, since the value genuinely can be explicit `undefined` there.

**Verified:** `pnpm typecheck` (`tsc --noEmit`) passes with zero errors. `pnpm build` (`tsc && vite build`) succeeds — pre-existing chunk-size/dynamic-import warnings are unrelated to this change, not addressed here.
