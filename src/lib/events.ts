/**
 * TASK-RT-001 (Doc 30, Doc 19 §15.1): a single source of truth for the
 * payload shapes of the app's centrally-emitted Tauri events, mirroring
 * `src-tauri/src/ipc/events.rs`'s `AppEvent` field-for-field.
 *
 * `ScanProgressPayload` and `SystemWarningPayload` already lived in
 * `@/lib/ipc` before this file existed (widely imported there already) --
 * re-exported here rather than duplicated, so this module is genuinely the
 * one place all event payload types can be imported from going forward.
 */
export type { ScanProgressPayload, SystemWarningPayload } from '@/lib/ipc';

// Mirrors ingestion/queues.rs's real `transaction_created` emission:
// `serde_json::json!({ "observation_id": obs_id })` -- deliberately minimal
// (Doc 19 §15.1 v1.14); a richer payload is TASK-RT-005's decision to make.
export interface TransactionCreatedPayload {
  observation_id: string;
}

// Mirrors commands/mod.rs's/ingestion/queues.rs's real `reconciliation_cluster`
// emission: `serde_json::json!({ "cluster_id": ..., "observation_id": ... })`.
export interface ReconciliationClusterPayload {
  cluster_id: string;
  observation_id: string;
}

// Mirrors `background_tasks::indicator::TaskProgress` (Rust) field-for-field.
export interface BackgroundTaskProgressPayload {
  task_id: string;
  task_type: string;
  label: string;
  current: number;
  total: number;
  eta_seconds: number | null;
  status: 'running' | 'completed' | 'failed';
  progress_pct: number;
  status_message: string;
}
