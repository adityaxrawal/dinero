# Synthetic Checks — Licensing Backend

Doc 30 TASK-OPS-003. The desktop app is local-first and single-tenant, so
there is nothing to synthetically poll on the desktop side beyond what the
app itself already reports via the local `get_health_report` IPC command
(see `src-tauri/src/health.rs`). The one thing worth external, continuous
monitoring is the Licensing Backend (Vercel + Neon), since it is shared
infrastructure every installed copy of the app depends on.

## Endpoint

`GET https://<licensing-backend-domain>/api/health`

No authentication required — this is deliberate. It exists specifically so
an external uptime monitor can poll it with zero account access and zero PII
exposure risk.

Response body, always exactly:

```json
{ "status": "ok", "db_latency_ms": 12 }
```

or, when the Neon connection fails:

```json
{ "status": "degraded", "db_latency_ms": 340 }
```

HTTP status is `200` for `ok`, `503` for `degraded`. The body never contains
an account email, license key, device fingerprint, JWT, or any other
identity/billing field — see `licensing-backend/api/health.ts`'s
`checkHealth()` and its test `test_license_health_endpoint_returns_minimal_metadata`
in `licensing-backend/tests/health.test.ts`.

## What to configure in an external monitor

- **Check**: `GET /api/health`, expect HTTP 200.
- **Frequency**: every 1–5 minutes (this is a single indexed `SELECT 1`
  through Prisma — cheap enough for continuous polling without adding load).
- **Alert threshold**: 2 consecutive failures (a single blip is not
  actionable; 2 in a row against a 1–5 minute interval is).
- **On alert**: follow `docs/incident-response.md`'s "validation outage"
  playbook (TASK-OPS-005) — the desktop app's 7-day offline grace period
  means a short Licensing Backend outage does not lock any user out
  immediately, which should shape the urgency of the response.

## Why no desktop-side synthetic check

The desktop app has no publicly reachable endpoint to poll — it is a local
Mac process with no listening network port. Its own health is instead
observable in-app via `get_health_report` (Settings → About, and the
existing `system_warning` banner for degraded conditions), and in aggregate
across the install base only through whatever the user chooses to export via
`db_export_diagnostic_bundle` (TASK-OPS-004). There is deliberately no
telemetry pipeline that would let Dinero itself synthetically monitor
installed copies — that would be a standing conflict with the local-first,
no-telemetry-by-default posture the rest of this codebase enforces.
