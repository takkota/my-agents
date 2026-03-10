use crate::domain::task::{AgentCli, Status};
use crate::services::tmux::TmuxService;
use crate::storage::FsStore;

pub struct AgentMonitor {
    store: FsStore,
    tmux: TmuxService,
}

pub enum MonitorEvent {
    StatusChanged { task_id: String, project_id: String, status: Status },
}

/// Signal file name written by Claude Code hooks in the task directory.
const SIGNAL_FILE: &str = ".agent_signal";

impl AgentMonitor {
    pub fn new(store: FsStore, tmux: TmuxService) -> Self {
        Self { store, tmux }
    }

    pub fn check_all(&self) -> Vec<MonitorEvent> {
        let mut events = Vec::new();
        let tasks = self.store.list_all_tasks().unwrap_or_default();

        for task in &tasks {
            if task.agent_cli == AgentCli::None {
                continue;
            }

            let event = match task.agent_cli {
                AgentCli::Claude => self.check_claude_task(&task.id, &task.project_id, &task.status, &task.tmux_session),
                AgentCli::Codex => self.check_codex_task(&task.id, &task.project_id, &task.status, &task.tmux_session),
                AgentCli::None => None,
            };

            if let Some(e) = event {
                events.push(e);
            }
        }

        events
    }

    /// Claude Code: check signal file written by hooks.
    /// - Signal file exists → InReview (agent stopped or idle)
    /// - Signal file absent + tmux session alive → InProgress (PreToolUse hook cleared it)
    fn check_claude_task(
        &self,
        task_id: &str,
        project_id: &str,
        current_status: &Status,
        tmux_session: &Option<String>,
    ) -> Option<MonitorEvent> {
        let signal_path = self.store.task_dir(project_id, task_id).join(SIGNAL_FILE);

        if signal_path.exists() {
            // Signal file present: agent stopped or idle
            if *current_status == Status::InProgress {
                return Some(MonitorEvent::StatusChanged {
                    task_id: task_id.to_string(),
                    project_id: project_id.to_string(),
                    status: Status::InReview,
                });
            }
        } else if *current_status == Status::InReview {
            // Signal file absent: only transition back to InProgress if the tmux
            // session is alive (proving the PreToolUse hook cleared the signal,
            // not that it was never created).
            let session_alive = tmux_session
                .as_deref()
                .is_some_and(|s| self.tmux.session_exists(s));
            if session_alive {
                return Some(MonitorEvent::StatusChanged {
                    task_id: task_id.to_string(),
                    project_id: project_id.to_string(),
                    status: Status::InProgress,
                });
            }
        }

        None
    }

    /// Codex: keep existing tmux-based polling.
    fn check_codex_task(
        &self,
        task_id: &str,
        project_id: &str,
        current_status: &Status,
        tmux_session: &Option<String>,
    ) -> Option<MonitorEvent> {
        let session_name = tmux_session.as_deref()?;
        if !self.tmux.session_exists(session_name) {
            return None;
        }

        let content = self.tmux.capture_pane(session_name).ok()?;
        let is_waiting = is_waiting_for_input_codex(&content);

        match (is_waiting, current_status) {
            (true, Status::InProgress) => Some(MonitorEvent::StatusChanged {
                task_id: task_id.to_string(),
                project_id: project_id.to_string(),
                status: Status::InReview,
            }),
            (false, Status::InReview) => Some(MonitorEvent::StatusChanged {
                task_id: task_id.to_string(),
                project_id: project_id.to_string(),
                status: Status::InProgress,
            }),
            _ => None,
        }
    }

}

fn is_waiting_for_input_codex(content: &str) -> bool {
    let last_lines: String = content
        .lines()
        .rev()
        .take(5)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");

    last_lines.contains("Approve?")
        || last_lines.contains("(y/n)")
        || last_lines.contains("> ")
}
