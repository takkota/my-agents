use crate::domain::task::{AgentCli, Status, TaskLink};
use crate::services::tmux::TmuxService;
use crate::storage::FsStore;

pub struct AgentMonitor {
    store: FsStore,
    tmux: TmuxService,
}

pub enum MonitorEvent {
    StatusChanged { task_id: String, project_id: String, status: Status },
    PrLinkDiscovered { task_id: String, project_id: String, url: String },
}

/// Signal file name written by Claude Code hooks in the task directory.
const SIGNAL_FILE: &str = ".agent_signal";

/// File where PostToolUse hook appends discovered PR URLs.
const PR_LINKS_FILE: &str = ".pr_links";

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

            // Check for PR links discovered by hooks (Claude only)
            if task.agent_cli == AgentCli::Claude {
                let link_events = self.check_pr_links(&task.id, &task.project_id, &task.links);
                events.extend(link_events);
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

    /// Check `.pr_links` file for PR URLs discovered by PostToolUse hook.
    /// Returns events for URLs not already present in the task's links.
    fn check_pr_links(
        &self,
        task_id: &str,
        project_id: &str,
        existing_links: &[TaskLink],
    ) -> Vec<MonitorEvent> {
        let pr_links_path = self.store.task_dir(project_id, task_id).join(PR_LINKS_FILE);

        if !pr_links_path.exists() {
            return Vec::new();
        }

        let content = match std::fs::read_to_string(&pr_links_path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };

        content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.trim().to_string())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .filter(|url| !existing_links.iter().any(|l| l.url == *url))
            .map(|url| MonitorEvent::PrLinkDiscovered {
                task_id: task_id.to_string(),
                project_id: project_id.to_string(),
                url,
            })
            .collect()
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
