# Implementation Log

Append-only. One line per task, appended only once the task is genuinely complete (code written, tested, committed). An unlogged task reads as `not started` to the next session by design.

| Task ID | Area | Status | Timestamp | Note |
|---|---|---|---|---|
| TASK-SETUP-001 | SETUP | done | 2026-07-14T00:00:00Z | Pre-existing scaffold matched spec; fixed `productName` "dinero-app" → "Dinero". |
| TASK-SETUP-002 | SETUP | done | 2026-07-14T00:05:00Z | Tightened CSP to spec: dropped direct WebView access to Google/GitHub domains, fixed licensing origin to license.dinero.app. |
| TASK-SETUP-003 | SETUP | done | 2026-07-14T00:10:00Z | No code change — Tauri v2's capability model already disables generic shell/fs/http for the WebView. |
| TASK-SETUP-004 | SETUP | done | 2026-07-14T00:12:00Z | Fixed window config: title, width, minWidth, transparent; added explicit resizable/decorations/center. |
| TASK-SETUP-005 | SETUP | done | 2026-07-14T00:14:00Z | Dependency set matches spec except sqlx (flagged for TASK-DB-001/002 — existing code uses rusqlite_migration). |
| TASK-SETUP-006 | SETUP | done | 2026-07-14T00:20:00Z | Implemented from scratch: src-tauri/src/startup.rs RAM check, llm_eligible state, system_warning event. |
| TASK-SETUP-007 | SETUP | done | 2026-07-14T00:25:00Z | tsconfig: ES2022, exactOptionalPropertyTypes, 4 missing path aliases (+ vite.config.ts mirror). Fixed 2 latent tsc errors so `tsc --noEmit` passes clean. |
| TASK-SETUP-008 | SETUP | done | 2026-07-14T00:30:00Z | no-explicit-any -> error, .prettierrc corrected to spec. FLAGGED: pnpm lint now reports 76 errors (pre-existing `any` usage in Area 9 pages, not fixed here — deferred to owning TASK-FE-* tasks). |
| TASK-SETUP-009 | SETUP | done | 2026-07-14T00:32:00Z | rust.yml already matches/exceeds spec. No change. |
| TASK-SETUP-010 | SETUP | done | 2026-07-14T00:33:00Z | react.yml already matches/exceeds spec. No change. FLAGGED: ESLint step will fail until TASK-SETUP-008's any-errors are fixed. |
| TASK-SETUP-011 | SETUP | done | 2026-07-14T00:34:00Z | benchmark.yml already matches spec; phase10_quality_gates_tests.rs verified to implement all 3 Doc 34 §10.2 thresholds. No change. Placeholder repo name flagged, not fixable now. |
| TASK-SETUP-012 | SETUP | done | 2026-07-14T00:45:00Z | Added Parse/Io/Internal/Validation variants + code() mapping; fixed real bug: Serialize emitted bare string, frontend expected {code,message} — every error was silently becoming UNKNOWN_ERROR. No From<AppError> impl needed (Tauri's own blanket Serialize->InvokeError impl covers it; writing one would conflict). |
| TASK-SETUP-013 | SETUP | done | 2026-07-14T01:00:00Z | Added ipc::with_panic_boundary (async-safe tokio::spawn/JoinError, not sync catch_unwind — not yet wired into existing commands, flagged for TASK-API-001). Added useIpcInvoke/useIpcListen hooks + src/types/ipc.ts (new, additive). |
| TASK-SETUP-014 | SETUP | done | 2026-07-14T01:05:00Z | PR/issue templates already matched spec exactly. No change. |
| TASK-SETUP-015 | SETUP | done | 2026-07-14T01:10:00Z | Created docs/dev-setup.md (OAuth dev setup, DB reset, privacy invariants); corrected README.md prerequisites (Node/pnpm/macOS versions, Xcode CLT, Keychain note). |
| TASK-DB-001 | DB | done | 2026-07-14T01:30:00Z | Fixed real Argon2 salt bug (hardcoded constant, not hw_uuid-derived); added auto_vacuum=INCREMENTAL + guarded VACUUM; renamed data.db->finance.db (4 sites). FLAGGED: cargo clippy/fmt both fail pre-existingly, codebase-wide (unrelated to this task) — rust.yml CI has likely never passed. |
| TASK-DB-002 | DB | done | 2026-07-14T03:00:00Z | User-directed: migrated rusqlite_migration -> sqlx::migrate! (19 migrations converted to timestamped .sql files). Verified SQLCipher compat empirically (shared libsqlite3-sys). Converted ~20 test files' setup_db() helpers from open_in_memory() to temp-file async migration (sqlx can't reach in-memory rusqlite connections). 313 tests pass, same 4 pre-existing unrelated failures. |
| TASK-DB-003 | DB | done | 2026-07-14T03:10:00Z | Table/CRUD/tests already matched spec. Fixed real bug: seed defaults were timezone='UTC', limit_thresholds='[]' instead of spec's 'Asia/Kolkata'/[80,90,100]. |
| TASK-DB-004 | DB | done | 2026-07-14T03:20:00Z | Table/CRUD/FK-violation tests already matched spec. Added missing UNIQUE constraint on email_address (new migration 020) - same Gmail account could previously be connected twice. |
