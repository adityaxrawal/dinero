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

---

## TASK-SETUP-008: Configure ESLint and Prettier

**Found:** The repo uses ESLint 9's flat config (`eslint.config.js`), not the `.eslintrc.cjs`/`.eslintignore` files Document 30 names — those formats don't exist for this ESLint version; flat config's own `ignores` array (already present: `{ ignores: ['dist'] }`) is the direct equivalent, and flat config has built-in default ignores for `node_modules` etc. Not a deviation, a tooling-generation difference (same situation as TASK-SETUP-003's Tauri v1→v2 allowlist terminology). `eslint-plugin-react-hooks`'s `recommended` config already sets `rules-of-hooks: error` and `exhaustive-deps: warn` by default — matches spec without change. Two real deviations: `@typescript-eslint/no-explicit-any` was `'warn'` (spec: `'error'`), and `.prettierrc` had `semi: true` (spec: no semicolons), `trailingComma: "all"` (spec: `"es5"`), and no `printWidth` (spec: `100`, default is 80).

**What I did:** Set `no-explicit-any` to `'error'` and rewrote `.prettierrc` to `{ semi: false, singleQuote: true, tabWidth: 2, printWidth: 100, trailingComma: "es5" }`, matching spec exactly.

**Flagged, not silently resolved:** running `pnpm lint` with `no-explicit-any: error` now reports **76 errors** (up from 6 pre-existing, unrelated `no-useless-assignment` errors in `Settings.tsx`/`Transactions.tsx`) because ~81 existing `any` usages across `src/pages/*.tsx` are promoted from warning to error. Fixing all of them properly requires per-usage type investigation across files that mostly belong to Area 9 (Dashboard, Instruments, Settings, Statements, Transactions, etc.) — well beyond a lint-config task's scope, and premature before those pages' own TASK-FE-* tasks are reached. Setting the rule to `error` now (matching spec) makes this pre-existing debt visible rather than silently tolerated; **`pnpm lint` will not run cleanly until each Area 9 page's own task fixes its `any` usages** — recommend fixing incrementally as each owning TASK-FE-* is reached, not as a mass fix now. Did **not** apply a project-wide Prettier reformat (`semi:false`/`trailingComma: es5` now differ from how the whole existing codebase is formatted) — that's a repo-wide, ~250-file diff disproportionate to a config-correctness task; the config now states the intended style, actual reformatting is a separate, explicit action if wanted.

**Verified:** `pnpm lint` runs (config loads, no crash) — reports 87 problems (76 errors, 11 warnings) as explained above, not a clean pass. This is a known, flagged gap, not an oversight.

---

## TASK-SETUP-009 / TASK-SETUP-010 / TASK-SETUP-011: GitHub Actions CI workflows

**Found:** All three already exist and substantially exceed spec. `rust.yml` (spec names `rust-ci.yml` — cosmetic filename difference only): `macos-latest` ✓, `cargo fmt --all -- --check` ✓, `cargo clippy --all-targets --all-features -- -D warnings` ✓, `cargo test --all-features` ✓, `Swatinem/rust-cache@v2` for `Cargo.lock`-keyed caching (a Rust-specific wrapper around `actions/cache`, functionally equivalent to spec's literal "`actions/cache`" mention) ✓ — plus bonus `cargo audit` and unsigned-build-check jobs beyond spec. `react.yml` (spec names `frontend-ci.yml`): `ubuntu-latest` ✓, Node 20 ✓ (pnpm caching, since the project standardized on pnpm not npm — same substitution class as elsewhere), `pnpm lint`/`pnpm typecheck` (spec's `npm run lint`/`npm run tsc -- --noEmit` equivalents) ✓ — plus bonus unit-test, `pnpm audit`, and axe-core WCAG steps beyond spec. `benchmark.yml`: cron `'0 2 * * *'` matches spec exactly ✓, clones a private benchmark-corpus repo via a scoped token (functionally equivalent to spec's "scoped deploy key"), runs `cargo test phase10_quality_gates` which (verified by reading `src-tauri/src/phase10_quality_gates_tests.rs`) implements all three Document 34 §10.2 thresholds exactly: field-accuracy ≥95% (`test_nfr_003_extraction_accuracy`), false-positive rate ≤0.1% (`test_nfr_004_false_positive_rate`), false-merge rate ≤0.1% (`test_nfr_005_false_merge_rate`).

**What I did:** No changes — all three match spec (or exceed it) on substance; only filenames differ cosmetically, not worth renaming and churning git history for.

**Flagged, not fixed:** (1) `no-explicit-any: error` (TASK-SETUP-008) means `react.yml`'s "Run ESLint" step will now fail CI until the ~81 pre-existing `any` usages are fixed by their owning Area 9 tasks — a direct, expected consequence already flagged under TASK-SETUP-008, cross-referenced here since this is the CI job it actually breaks. (2) `benchmark.yml` clones `your-org/dinero-benchmarks` — an obvious placeholder since no real benchmark-corpus repo has been provisioned yet (Document 34 §7's corpus is a future asset); not fixable now, needs a real org/repo once provisioned.

**Verified:** Read all three workflow files in full; cross-checked `phase10_quality_gates_tests.rs`'s actual threshold values against Document 34 §10.2.

---

## TASK-SETUP-012: Define AppError Enum and IPC Error Plumbing

**Found:** `src-tauri/src/error.rs` (doc names `errors.rs`, plural — cosmetic) already had `Db`, `Network`, `Auth`, `LicenseLocked(String)`, plus two non-spec variants (`Unknown`, `FileAccessDenied`) used extensively — `AppError::Unknown` alone has 72 call sites, `AppError::Db` 62, across 6 files (`commands/mod.rs`, `commands/debug.rs`, `licensing/commands.rs`, `licensing/gate.rs`, `ingestion/historical_scan.rs`). Spec's `Parse`/`Io`/`Internal`/`Validation` variants were entirely absent. Spec says `LicenseLocked` should be a unit variant; existing code carries a message and has 9 call sites.

**Real bug found (not just a naming gap):** the existing `Serialize` impl emitted a bare JSON string (`"Database error: ..."`), but `src/lib/ipc.ts`'s `invokeCommand()` wrapper — already written, already in use — explicitly checks `'code' in error && 'message' in error` (Document 19 §3.4's structured contract) and falls through to a generic `UNKNOWN_ERROR` when that check fails. Since a bare string never satisfies `'code' in error`, **every single Rust command error was silently arriving at the frontend as `UNKNOWN_ERROR`, losing its real code and message entirely** — a genuinely broken contract, not a stylistic mismatch.

**What I did:** (1) Added `Parse`, `Io`, `Internal`, `Validation` variants additively — zero call-site breakage. (2) Left `Unknown`, `FileAccessDenied`, and `LicenseLocked(String)`'s message untouched — renaming/removing them would require migrating 90+ call sites across 6 files against Document 19 §4's full ~25-code catalog, which is explicitly **TASK-API-010's scope** ("Standardized Error Response Contract Across All Commands"), not this setup task's; flagged for that task rather than resolved here. (3) Added `AppError::code()` mapping each variant to one of Document 19 §4's existing generic codes (`INTERNAL_ERROR`, `NETWORK_ERROR`, `UNAUTHORIZED`, `LICENSE_LOCKED`, `VALIDATION_ERROR`) — deliberately not inventing new codes or assigning the catalog's many domain-specific codes (`SCAN_NOT_FOUND`, `CLUSTER_NOT_FOUND`, etc.), which only make sense assigned per-command. (4) Rewrote `Serialize` to emit `{ "code": ..., "message": ... }`, fixing the real contract bug above. (5) Did **not** write `impl From<AppError> for tauri::ipc::InvokeError` as the doc's task text says to — checked Tauri 2.11.3's actual source (`tauri-2.11.3/src/ipc/mod.rs`): it has `impl<T: Serialize> From<T> for InvokeError`, a blanket impl already covering every `Serialize` type. Writing a second, overlapping impl for `AppError` specifically would be a Rust coherence violation (E0119, conflicting implementations) and fail to compile — the doc's requirement is already satisfied automatically by `AppError: Serialize`.

**Verified:** `cargo check` clean. 10 new/updated unit tests in `error.rs`, one per variant, asserting the exact `{code, message}` JSON shape — all pass. Ran the full `cargo test --lib` suite before and after (by temporarily reverting `error.rs` and re-running) to confirm the 4 pre-existing failures (`commands::data::tests::test_seed_and_fetch` — unrelated DB schema/column drift; three `phase10_rigorous_tests` — missing fixture files on disk) are identical with and without this change, i.e. genuinely pre-existing and not introduced here. 309 passed both times.

---

## TASK-SETUP-013: Build Core IPC Framework — Typed Structs and React Hooks

**Found:** `src-tauri/src/ipc/responses.rs` already had `Payload<T> { data: Option<T>, error: Option<String> }` — functionally identical to the spec's `IpcResponse<T>`, just named differently (not renamed, would touch existing call sites for no behavioral gain). `src-tauri/src/ipc/args.rs` already has several typed argument structs with `Serialize`/`Deserialize`/`Debug` derives. What was genuinely missing: (1) any global IPC panic boundary at all (`catch_unwind` only appears once, narrowly, inside `extraction/llm.rs` as an OOM guard for the local LLM — not a command-wide boundary); (2) both React hooks (`useIpcInvoke`, `useIpcListen`) — didn't exist; existing components (`AppLayout.tsx`, `GlobalStateContext.tsx`, `Transactions.tsx`, `SpendingLimits.tsx`) each hand-roll their own `useEffect` + `listen()`/`unlisten()` boilerplate instead; (3) `src/types/ipc.ts` — didn't exist; `AppError`'s shape was a private, unexported `interface` duplicated only inside `src/lib/ipc.ts`.

**Async-safety correction (flagged, not silently followed):** Document 30's task text says implement the boundary via `std::panic::catch_unwind`, matching Document 19 §3.4. That mechanism only catches *synchronous* panics inside its own closure and does not work across `.await` points — but nearly every command in this codebase is `async fn`. Implemented the actual async-safe equivalent instead: `ipc::with_panic_boundary<F, T>()` spawns the future via `tokio::spawn` and inspects the resulting `JoinError::is_panic()`, which Tokio guarantees isolates a panicking task and reports it back as an `Err` — the correct analog to `catch_unwind` for this codebase's real (async) command shapes. Maps a caught panic to `AppError::Internal` per spec; logs via `tracing::error!` (flows into `app-logs.log`, which the diagnostic bundle export already reads) rather than writing an `audit_log` row directly, since the latter would require threading a DB pool into this generic, DB-agnostic primitive.

**What I did:** Implemented `with_panic_boundary()` in `src-tauri/src/ipc/mod.rs` (new file content — 3 unit tests: ok-passthrough, err-passthrough, actual-panic-caught-and-mapped). **Not wired into any of the ~53 existing command handlers** — retrofitting every already-built command (`commands/mod.rs`, `licensing/commands.rs`, etc.) is a wide, invasive change spanning most of Area 8's already-built IPC surface; flagged for TASK-API-001 ("IPC Request Validation Middleware") as the natural integration point, since a DB pool is already in scope there for the `audit_log` write too. Created `src/types/ipc.ts` (canonical `AppError`, plus `IpcResponse<T>` mirroring the Rust `Payload<T>`); updated `src/lib/ipc.ts` to import `AppError` from there instead of its own private copy, and exported its existing `invokeCommand()` helper so the new hook can reuse it rather than duplicate the error-normalization logic. Added `src/hooks/useIpcInvoke.ts` and `src/hooks/useIpcListen.ts` — both additive; existing call sites are not migrated to use them (out of scope, would touch several unrelated component files for a non-functional refactor).

**Verified:** `cargo check` clean; 3 new Rust unit tests pass; full `cargo test --lib` still shows 312 passed (up from 309 — the 3 new ones) against the same 4 pre-existing, unrelated failures. `pnpm typecheck` and `pnpm build` both pass clean with the new files and the `lib/ipc.ts` changes.
