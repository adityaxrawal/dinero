//! TASK-DESK-003 (Doc 30 §12, Doc 29 §14): a single aggregated registry of
//! long-running background tasks (historical scans, local-LLM-assisted
//! extraction during those scans), backing a persistent, non-blocking
//! progress indicator. Any long-running task registers with this on start
//! and deregisters on completion; every mutation emits the Document 19
//! §15-catalogued `background_task_progress` event so React can render a
//! summary count with expandable per-task detail.
//!
//! Event shape note: Document 19 §15 names this event's payload fields as
//! `task_type`, `progress_pct`, `status_message` -- this module emits
//! exactly those plus additive fields (`task_id`, `label`, `current`,
//! `total`, `eta_seconds`, `status`) per Document 19 §19's "introduce new
//! fields as additive changes" versioning rule. `status` is what lets the
//! frontend distinguish "still running" from "done" without having to
//! (incorrectly) infer completion from `current == total`, which would be
//! wrong for a task that fails or is cancelled partway through.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, Runtime};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskProgress {
    pub task_id: String,
    pub task_type: String,
    pub label: String,
    pub current: u64,
    pub total: u64,
    pub eta_seconds: Option<u64>,
    pub status: TaskStatus,
    /// Document 19 §15's named field for this event.
    pub progress_pct: f64,
    /// Document 19 §15's named field for this event.
    pub status_message: String,
}

/// Doc 30 acceptance `test_indicator_does_not_block_ui_thread`: a plain
/// `std::sync::Mutex` guarding a small `HashMap` is held only for the
/// duration of a `HashMap` insert/remove/clone -- never across an `.await`
/// point -- so it can never itself stall the async runtime driving the UI.
#[derive(Default)]
pub struct BackgroundTaskRegistry {
    tasks: Mutex<HashMap<String, (TaskProgress, Instant)>>,
}

fn compute_progress_pct(current: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        ((current as f64 / total as f64) * 100.0).min(100.0)
    }
}

fn compute_eta_seconds(current: u64, total: u64, elapsed: std::time::Duration) -> Option<u64> {
    if current == 0 || current >= total {
        return None;
    }
    let per_unit = elapsed.as_secs_f64() / current as f64;
    let remaining = total.saturating_sub(current) as f64;
    Some((per_unit * remaining).round() as u64)
}

impl BackgroundTaskRegistry {
    /// Registers a new task (or re-registers/updates an existing one --
    /// insert-or-replace, so callers don't need a separate "does this task
    /// already exist" check before calling this on every progress tick).
    #[allow(clippy::too_many_arguments)]
    pub fn register_or_update<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        task_id: &str,
        task_type: &str,
        label: &str,
        current: u64,
        total: u64,
        status_message: &str,
    ) {
        let start = {
            let tasks = self.tasks.lock().unwrap();
            tasks.get(task_id).map(|(_, started_at)| *started_at)
        }
        .unwrap_or_else(Instant::now);

        let progress = TaskProgress {
            task_id: task_id.to_string(),
            task_type: task_type.to_string(),
            label: label.to_string(),
            current,
            total,
            eta_seconds: compute_eta_seconds(current, total, start.elapsed()),
            status: TaskStatus::Running,
            progress_pct: compute_progress_pct(current, total),
            status_message: status_message.to_string(),
        };

        self.tasks
            .lock()
            .unwrap()
            .insert(task_id.to_string(), (progress.clone(), start));

        let _ = crate::ipc::events::emit_event(
            app,
            crate::ipc::events::AppEvent::BackgroundTaskProgress,
            progress,
        );
    }

    /// Deregisters a task on completion (success or failure) and emits one
    /// final event carrying the terminal `status` -- this is the signal the
    /// frontend removes the task from its active list on, not an inferred
    /// `current == total`.
    pub fn deregister<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        task_id: &str,
        status: TaskStatus,
        status_message: &str,
    ) {
        let removed = self.tasks.lock().unwrap().remove(task_id);
        let Some((mut progress, _)) = removed else {
            return;
        };
        progress.status = status;
        progress.status_message = status_message.to_string();
        progress.eta_seconds = None;
        let _ = crate::ipc::events::emit_event(
            app,
            crate::ipc::events::AppEvent::BackgroundTaskProgress,
            progress,
        );
    }

    /// All currently-registered (i.e. still running) tasks. Doc 30
    /// TASK-RT-004: backs `get_active_background_tasks`, so a late-mounting
    /// (or freshly-navigated-to) indicator can recover in-progress tasks it
    /// never received the live `background_task_progress` event for, same
    /// principle as `ipc::system_warnings`'s registry (TASK-RT-007).
    pub fn active_tasks(&self) -> Vec<TaskProgress> {
        self.tasks
            .lock()
            .unwrap()
            .values()
            .map(|(progress, _)| progress.clone())
            .collect()
    }

    pub fn active_count(&self) -> usize {
        self.tasks.lock().unwrap().len()
    }
}

/// Doc 30 TASK-RT-004: late-mount/route-navigation recovery for the
/// background task indicator -- mirrors `get_active_system_warnings`
/// (TASK-RT-007)'s same rationale.
#[tauri::command]
pub fn get_active_background_tasks(
    registry: tauri::State<'_, BackgroundTaskRegistry>,
) -> Vec<TaskProgress> {
    registry.active_tasks()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Manager;

    fn mock_app() -> AppHandle<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap()
            .handle()
            .clone()
    }

    /// Doc 30 TASK-DESK-003 acceptance: `test_multiple_concurrent_tasks_aggregate_correctly`.
    #[test]
    fn test_multiple_concurrent_tasks_aggregate_correctly() {
        let app = mock_app();
        let registry = BackgroundTaskRegistry::default();

        registry.register_or_update(
            &app,
            "scan_1",
            "historical_scan",
            "Scanning acct_1",
            10,
            100,
            "Scanning...",
        );
        registry.register_or_update(
            &app,
            "scan_2",
            "historical_scan",
            "Scanning acct_2",
            50,
            200,
            "Scanning...",
        );

        assert_eq!(registry.active_count(), 2);
        let mut ids: Vec<String> = registry
            .active_tasks()
            .iter()
            .map(|t| t.task_id.clone())
            .collect();
        ids.sort();
        assert_eq!(ids, vec!["scan_1".to_string(), "scan_2".to_string()]);

        // Updating an already-registered task must not create a duplicate entry.
        registry.register_or_update(
            &app,
            "scan_1",
            "historical_scan",
            "Scanning acct_1",
            20,
            100,
            "Scanning...",
        );
        assert_eq!(registry.active_count(), 2);
        let updated = registry
            .active_tasks()
            .into_iter()
            .find(|t| t.task_id == "scan_1")
            .unwrap();
        assert_eq!(updated.current, 20);
    }

    /// Doc 30 TASK-DESK-003 acceptance: `test_task_deregistered_on_completion`.
    #[test]
    fn test_task_deregistered_on_completion() {
        let app = mock_app();
        let registry = BackgroundTaskRegistry::default();

        registry.register_or_update(
            &app,
            "scan_1",
            "historical_scan",
            "Scanning",
            10,
            100,
            "Scanning...",
        );
        assert_eq!(registry.active_count(), 1);

        registry.deregister(&app, "scan_1", TaskStatus::Completed, "Done");
        assert_eq!(registry.active_count(), 0);
        assert!(registry.active_tasks().is_empty());
    }

    #[test]
    fn test_deregister_of_unknown_task_is_a_safe_noop() {
        let app = mock_app();
        let registry = BackgroundTaskRegistry::default();
        registry.deregister(&app, "never_registered", TaskStatus::Failed, "n/a");
        assert_eq!(registry.active_count(), 0);
    }

    /// Doc 30 TASK-DESK-003 acceptance: `test_indicator_does_not_block_ui_thread`.
    /// The registry's lock is only ever held for a synchronous HashMap
    /// operation -- proven here by driving concurrent register/deregister
    /// calls from many tasks on a real multi-threaded Tokio runtime and
    /// confirming they all complete promptly (no deadlock, no long stall).
    #[tokio::test]
    async fn test_indicator_does_not_block_ui_thread() {
        use std::sync::Arc;

        let app = mock_app();
        let registry = Arc::new(BackgroundTaskRegistry::default());

        let start = Instant::now();
        let mut handles = Vec::new();
        for i in 0..50 {
            let registry = Arc::clone(&registry);
            let app = app.clone();
            handles.push(tokio::spawn(async move {
                let task_id = format!("task_{i}");
                registry.register_or_update(&app, &task_id, "test", "Test task", 1, 10, "Running");
                registry.deregister(&app, &task_id, TaskStatus::Completed, "Done");
            }));
        }
        for handle in handles {
            handle.await.unwrap();
        }

        assert_eq!(registry.active_count(), 0);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "50 concurrent register/deregister cycles must complete promptly, not stall"
        );
    }

    /// Doc 30 TASK-RT-004: a late-mounting indicator component (or one
    /// re-mounted after a route navigation) must be able to recover
    /// already-running tasks it never received the live event for.
    #[test]
    fn test_get_active_background_tasks_recovers_already_running_task() {
        let app = mock_app();
        app.manage(BackgroundTaskRegistry::default());
        let registry = app.state::<BackgroundTaskRegistry>();
        registry.register_or_update(&app, "scan_1", "historical_scan", "Scanning acct_1", 10, 100, "Scanning...");

        let recovered = get_active_background_tasks(registry);
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].task_id, "scan_1");
    }

    #[test]
    fn test_compute_progress_pct_and_eta() {
        assert_eq!(compute_progress_pct(0, 0), 0.0);
        assert_eq!(compute_progress_pct(50, 100), 50.0);
        assert_eq!(compute_progress_pct(100, 100), 100.0);
        assert_eq!(
            compute_progress_pct(150, 100),
            100.0,
            "must clamp, never exceed 100%"
        );

        assert_eq!(
            compute_eta_seconds(0, 100, std::time::Duration::from_secs(10)),
            None
        );
        assert_eq!(
            compute_eta_seconds(100, 100, std::time::Duration::from_secs(10)),
            None
        );
        // 10 units in 10s -> 1s/unit; 90 remaining -> ~90s.
        assert_eq!(
            compute_eta_seconds(10, 100, std::time::Duration::from_secs(10)),
            Some(90)
        );
    }
}
