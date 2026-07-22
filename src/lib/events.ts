/**
 * TASK-RT-001 (Doc 30, Doc 19 §15.1): a single source of truth for the
 * payload shapes of the app's centrally-emitted Tauri events, mirroring
 * `src-tauri/src/ipc/events.rs`'s `AppEvent` field-for-field.
 *
 * `ScanProgressPayload`, `SystemWarningPayload`, and
 * `BackgroundTaskProgressPayload` already lived in `@/lib/ipc` before this
 * file existed (widely imported there already, `BackgroundTaskProgressPayload`
 * added for TASK-RT-004's `get_active_background_tasks` wrapper) --
 * re-exported here rather than duplicated, so this module is genuinely the
 * one place all event payload types can be imported from going forward.
 */
export type {
  ScanProgressPayload,
  SystemWarningPayload,
  BackgroundTaskProgressPayload,
} from '@/lib/ipc';

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

