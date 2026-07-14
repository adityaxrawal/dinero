# Implementation Log

Append-only. One line per task, appended only once the task is genuinely complete (code written, tested, committed). An unlogged task reads as `not started` to the next session by design.

| Task ID | Area | Status | Timestamp | Note |
|---|---|---|---|---|
| TASK-SETUP-001 | SETUP | done | 2026-07-14T00:00:00Z | Pre-existing scaffold matched spec; fixed `productName` "dinero-app" → "Dinero". |
| TASK-SETUP-002 | SETUP | done | 2026-07-14T00:05:00Z | Tightened CSP to spec: dropped direct WebView access to Google/GitHub domains, fixed licensing origin to license.dinero.app. |
| TASK-SETUP-003 | SETUP | done | 2026-07-14T00:10:00Z | No code change — Tauri v2's capability model already disables generic shell/fs/http for the WebView. |
| TASK-SETUP-004 | SETUP | done | 2026-07-14T00:12:00Z | Fixed window config: title, width, minWidth, transparent; added explicit resizable/decorations/center. |
