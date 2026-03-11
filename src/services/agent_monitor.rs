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

/// File where PostToolUse hook appends discovered PR URLs.
const PR_LINKS_FILE: &str = ".pr_links";

/// Marker file created by UserPromptSubmit hook when the user sends a prompt.
/// Signals that the user is actively working in the session.
const PROMPT_SUBMITTED_FILE: &str = ".prompt_submitted";

/// Marker file created by Stop hook (Claude) or notify script (Codex) when
/// the agent finishes responding and is waiting for user input.
const AGENT_STOPPED_FILE: &str = ".agent_stopped";

impl AgentMonitor {
    pub fn new(store: FsStore, tmux: TmuxService) -> Self {
        Self { store, tmux }
    }

    pub fn check_all(&self) -> Vec<MonitorEvent> {
        let mut events = Vec::new();
        let tasks = self.store.list_all_tasks().unwrap_or_default();

        for task in &tasks {
            match task.agent_cli {
                AgentCli::Claude | AgentCli::Codex => {
                    if let Some(e) = self.check_agent_task(&task.id, &task.project_id, &task.status, &task.tmux_session) {
                        events.push(e);
                    }
                }
                AgentCli::None => {}
            }

            // PR link discovery is Claude-only (uses PostToolUse hook)
            if task.agent_cli == AgentCli::Claude {
                let link_events = self.check_pr_links(&task.id, &task.project_id, &task.links);
                events.extend(link_events);
            }
        }

        events
    }

    /// Check task status based on tmux session and marker files.
    ///
    /// Transitions:
    /// - (Todo|Completed|ActionRequired) + `.prompt_submitted` + session alive → InProgress
    /// - InProgress + `.agent_stopped` + session alive → ActionRequired
    /// - Todo + tmux session dead → Blocked (agent crashed or failed to start)
    ///
    /// Marker files:
    /// - `.prompt_submitted` — created by UserPromptSubmit hook (Claude) or
    ///   notify script (Codex) when the user sends a prompt.
    /// - `.agent_stopped` — created by Stop hook (Claude) or notify script
    ///   (Codex) when the agent finishes and is waiting for user input.
    fn check_agent_task(
        &self,
        task_id: &str,
        project_id: &str,
        current_status: &Status,
        tmux_session: &Option<String>,
    ) -> Option<MonitorEvent> {
        let task_dir = self.store.task_dir(project_id, task_id);
        let prompt_submitted_path = task_dir.join(PROMPT_SUBMITTED_FILE);
        let agent_stopped_path = task_dir.join(AGENT_STOPPED_FILE);
        let session_alive = tmux_session
            .as_deref()
            .is_some_and(|s| self.tmux.session_exists(s));

        // Priority 1: `.prompt_submitted` transitions idle states to InProgress.
        // ActionRequired is included so that re-engaging the agent moves it back.
        if matches!(current_status, Status::Todo | Status::Completed | Status::ActionRequired) {
            if session_alive && prompt_submitted_path.exists() {
                return Some(MonitorEvent::StatusChanged {
                    task_id: task_id.to_string(),
                    project_id: project_id.to_string(),
                    status: Status::InProgress,
                });
            }
            // Todo-specific: detect crashed/dead sessions
            if *current_status == Status::Todo && !session_alive && tmux_session.is_some() {
                return Some(MonitorEvent::StatusChanged {
                    task_id: task_id.to_string(),
                    project_id: project_id.to_string(),
                    status: Status::Blocked,
                });
            }
        }

        // Priority 2: `.agent_stopped` transitions InProgress to ActionRequired.
        if *current_status == Status::InProgress && session_alive && agent_stopped_path.exists() {
            return Some(MonitorEvent::StatusChanged {
                task_id: task_id.to_string(),
                project_id: project_id.to_string(),
                status: Status::ActionRequired,
            });
        }

        None
    }

    /// Check `.pr_links` file for PR URLs discovered by PostToolUse hook.
    /// Returns events for URLs not already present in the task's links.
    /// Removes consumed entries from the file to prevent unbounded growth.
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

        let urls: Vec<String> = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.trim().to_string())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .filter(|url| is_github_pr_url(url))
            .collect();

        let new_urls: Vec<String> = urls
            .iter()
            .filter(|url| !existing_links.iter().any(|l| l.url == **url))
            .cloned()
            .collect();

        // Remove consumed entries: keep only URLs already in task links
        // (i.e., not new). Any URL written concurrently by the hook will
        // survive because it won't be in existing_links yet.
        if !new_urls.is_empty() {
            let remaining: Vec<&str> = urls
                .iter()
                .filter(|url| existing_links.iter().any(|l| l.url == **url))
                .map(|s| s.as_str())
                .collect();
            if remaining.is_empty() {
                let _ = std::fs::remove_file(&pr_links_path);
            } else {
                let _ = std::fs::write(&pr_links_path, remaining.join("\n") + "\n");
            }
        }

        new_urls
            .into_iter()
            .map(|url| MonitorEvent::PrLinkDiscovered {
                task_id: task_id.to_string(),
                project_id: project_id.to_string(),
                url,
            })
            .collect()
    }

}

/// Validate that a string is a well-formed GitHub PR URL.
fn is_github_pr_url(url: &str) -> bool {
    let url = url.trim_end_matches('/');
    let parts: Vec<&str> = url.split('/').collect();
    if parts.len() < 5 {
        return false;
    }
    let len = parts.len();
    parts[len - 2] == "pull"
        && parts[len - 1].chars().all(|c| c.is_ascii_digit())
        && url.starts_with("https://github.com/")
}
