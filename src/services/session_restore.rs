use crate::domain::task::{AgentCli, Status};
use crate::services::tmux::TmuxService;
use crate::storage::FsStore;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// Minimal info needed to identify a restore target.
/// We intentionally do NOT carry the full Task to avoid writing stale data.
struct RestoreTarget {
    task_id: String,
    project_id: String,
    agent_cli: AgentCli,
    task_dir: PathBuf,
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

/// Build the resume command for the given agent CLI and task directory.
///
/// For Codex, `resume --last` picks the globally most recent session regardless
/// of cwd, so we scan `~/.codex/sessions` to find the latest session whose cwd
/// matches the task directory and use `codex resume <session_id>` instead.
/// If no matching session is found, falls back to the standard resume command.
///
/// For Claude and Gemini, the built-in resume commands are already cwd-based.
fn build_resume_command(cli: AgentCli, task_dir: &Path) -> Option<String> {
    match cli {
        AgentCli::Codex => {
            if let Some(session_id) = find_codex_session_for_cwd(task_dir) {
                Some(format!("codex resume {}", session_id))
            } else {
                // No matching session found — fall back to standard resume
                cli.resume_command()
            }
        }
        _ => cli.resume_command(),
    }
}

/// Scan `~/.codex/sessions` to find the most recent session whose cwd matches
/// or is a parent of `target_cwd`. Returns the session UUID if found.
///
/// Session files are JSONL; the first line contains `session_meta` with `cwd` and `id`.
/// Directory structure: `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`
fn find_codex_session_for_cwd(target_cwd: &Path) -> Option<String> {
    let sessions_dir = dirs::home_dir()?.join(".codex").join("sessions");
    if !sessions_dir.exists() {
        return None;
    }

    // Collect all .jsonl files with their paths
    let mut session_files: Vec<PathBuf> = Vec::new();
    collect_jsonl_files(&sessions_dir, &mut session_files);

    // Sort by filename descending (filenames contain timestamps, so lexicographic
    // order = chronological order). We want the most recent match.
    session_files.sort_by(|a, b| {
        b.file_name()
            .cmp(&a.file_name())
    });

    let target_cwd_str = target_cwd.to_string_lossy();

    for path in &session_files {
        if let Some((id, cwd)) = parse_session_meta(path) {
            // Match if the session's cwd is exactly the task dir or a subdirectory
            // (worktree dirs are children of the task dir)
            if cwd == target_cwd_str || cwd.starts_with(&format!("{}/", target_cwd_str)) {
                return Some(id);
            }
        }
    }

    None
}

/// Recursively collect all .jsonl files under a directory.
fn collect_jsonl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "jsonl") {
            out.push(path);
        }
    }
}

/// Parse the first line of a Codex session JSONL file to extract `id` and `cwd`
/// from the `session_meta` payload.
fn parse_session_meta(path: &Path) -> Option<(String, String)> {
    use std::io::{BufRead, BufReader};

    let file = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    let first_line = reader.lines().next()?.ok()?;

    let value: serde_json::Value = serde_json::from_str(&first_line).ok()?;
    if value.get("type")?.as_str()? != "session_meta" {
        return None;
    }
    let payload = value.get("payload")?;
    let id = payload.get("id")?.as_str()?.to_string();
    let cwd = payload.get("cwd")?.as_str()?.to_string();
    Some((id, cwd))
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

    // Build the appropriate resume command (cwd-aware for Codex)
    if let Some(cmd) = build_resume_command(target.agent_cli, &target.task_dir) {
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
