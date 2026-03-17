use crate::domain::task::{AgentCli, Status, Task};
use crate::services::tmux::TmuxService;
use crate::storage::FsStore;
use std::sync::mpsc;

/// Information about a task whose tmux session needs to be restored.
struct RestoreTarget {
    task: Task,
    task_dir: std::path::PathBuf,
    session_name: String,
}

/// Result of a single session restore attempt.
pub struct SessionRestoreResult {
    pub success: bool,
}

/// Spawn a background thread that restores tmux sessions for tasks that
/// previously had an agent CLI running. Returns a receiver for results.
///
/// Only restores tasks where:
/// - `agent_launched` is true (agent was previously launched)
/// - `agent_cli` is not None
/// - `tmux_session` was set (session existed before reboot)
/// - The tmux session no longer exists (lost after reboot)
/// - Task status is not Completed
pub fn restore_sessions_async(
    store: &FsStore,
    tmux: &TmuxService,
) -> Option<mpsc::Receiver<Vec<SessionRestoreResult>>> {
    if !TmuxService::is_available() {
        return None;
    }

    // Collect existing tmux sessions
    let existing_sessions: std::collections::HashSet<String> = tmux
        .list_sessions()
        .unwrap_or_default()
        .into_iter()
        .collect();

    // Find tasks that need session restore
    let all_tasks = store.list_all_tasks().unwrap_or_default();
    let targets: Vec<RestoreTarget> = all_tasks
        .into_iter()
        .filter(|task| {
            task.agent_launched
                && task.agent_cli != AgentCli::None
                && task.status != Status::Completed
                && task.tmux_session.is_some()
                && !existing_sessions.contains(task.tmux_session.as_deref().unwrap_or(""))
        })
        .map(|task| {
            let task_dir = store.task_dir(&task.project_id, &task.id);
            let session_name = task
                .tmux_session
                .clone()
                .unwrap_or_else(|| TmuxService::session_name(&task.project_id, &task.id));
            RestoreTarget {
                task,
                task_dir,
                session_name,
            }
        })
        .filter(|t| t.task_dir.exists())
        .collect();

    if targets.is_empty() {
        return None;
    }

    let store_clone = store.clone();
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let tmux = TmuxService::new();
        let mut results = Vec::new();

        for target in targets {
            let success = restore_single_session(&tmux, &store_clone, &target);
            results.push(SessionRestoreResult { success });
        }

        let _ = tx.send(results);
    });

    Some(rx)
}

/// Restore a single tmux session and launch the agent with resume command.
fn restore_single_session(tmux: &TmuxService, store: &FsStore, target: &RestoreTarget) -> bool {
    // Create the tmux session
    if tmux
        .create_session(&target.session_name, &target.task_dir)
        .is_err()
    {
        return false;
    }

    // Launch agent with resume command to continue previous conversation
    if let Some(cmd) = target.task.agent_cli.resume_command() {
        if tmux.send_text(&target.session_name, &cmd).is_err() {
            return false;
        }
    }

    // Save task to ensure tmux_session is persisted
    let mut task = target.task.clone();
    task.tmux_session = Some(target.session_name.clone());
    let _ = store.save_task(&task);

    true
}
