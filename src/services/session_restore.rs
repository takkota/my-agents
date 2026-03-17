use crate::domain::task::{AgentCli, Status};
use crate::services::tmux::TmuxService;
use crate::storage::FsStore;
use std::sync::mpsc;

/// Minimal info needed to identify a restore target.
/// We intentionally do NOT carry the full Task to avoid writing stale data.
struct RestoreTarget {
    task_id: String,
    project_id: String,
    agent_cli: AgentCli,
    task_dir: std::path::PathBuf,
    session_name: String,
}

/// Result of a single session restore attempt.
#[allow(dead_code)]
pub struct SessionRestoreResult {
    pub task_id: String,
    pub success: bool,
    pub error: Option<String>,
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

    // Find tasks that need session restore — extract only the IDs and metadata
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
                task_id: task.id,
                project_id: task.project_id,
                agent_cli: task.agent_cli,
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
            let (success, error) = restore_single_session(&tmux, &store_clone, &target);
            results.push(SessionRestoreResult {
                task_id: target.task_id.clone(),
                success,
                error,
            });
        }

        let _ = tx.send(results);
    });

    Some(rx)
}

/// Restore a single tmux session and launch the agent with resume command.
/// Returns (success, optional error message).
fn restore_single_session(
    tmux: &TmuxService,
    store: &FsStore,
    target: &RestoreTarget,
) -> (bool, Option<String>) {
    // Create the tmux session
    if let Err(e) = tmux.create_session(&target.session_name, &target.task_dir) {
        return (false, Some(format!("Failed to create session: {}", e)));
    }

    // Launch agent with resume command to continue previous conversation
    if let Some(cmd) = target.agent_cli.resume_command() {
        if let Err(e) = tmux.send_text(&target.session_name, &cmd) {
            // Clean up the empty session on failure
            let _ = tmux.kill_session(&target.session_name);
            return (false, Some(format!("Failed to launch agent: {}", e)));
        }
    }

    // Re-read the latest task.json to avoid overwriting concurrent changes,
    // then update only the tmux_session field.
    match store.list_tasks(&target.project_id) {
        Ok(tasks) => {
            if let Some(mut task) = tasks.into_iter().find(|t| t.id == target.task_id) {
                // Re-check: if status was changed to Completed while we were restoring,
                // kill the session we just created and bail.
                if task.status == Status::Completed {
                    let _ = tmux.kill_session(&target.session_name);
                    return (false, Some("Task was completed during restore".to_string()));
                }
                task.tmux_session = Some(target.session_name.clone());
                if let Err(e) = store.save_task(&task) {
                    return (
                        true,
                        Some(format!("Session restored but failed to save task: {}", e)),
                    );
                }
            }
        }
        Err(e) => {
            return (
                true,
                Some(format!("Session restored but failed to re-read task: {}", e)),
            );
        }
    }

    (true, None)
}
