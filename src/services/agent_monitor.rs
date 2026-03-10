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
            if task.agent_cli != AgentCli::Claude {
                continue;
            }

            if let Some(e) = self.check_claude_task(&task.id, &task.project_id, &task.status, &task.tmux_session) {
                events.push(e);
            }

            let link_events = self.check_pr_links(&task.id, &task.project_id, &task.links);
            events.extend(link_events);
        }

        events
    }

    /// Claude Code: check signal file written by hooks.
    /// - Signal file exists + InProgress → InReview (agent stopped or idle)
    /// - Signal file absent + tmux session alive → InProgress (PreToolUse hook cleared it)
    /// - Todo + tmux session alive → InProgress (stale signal is cleared first)
    /// - Todo + tmux session dead → Blocked (agent crashed or failed to start)
    /// Note: Todo → InReview is NOT allowed; Todo can only transition to InProgress or Blocked.
    fn check_claude_task(
        &self,
        task_id: &str,
        project_id: &str,
        current_status: &Status,
        tmux_session: &Option<String>,
    ) -> Option<MonitorEvent> {
        let signal_path = self.store.task_dir(project_id, task_id).join(SIGNAL_FILE);
        let session_alive = tmux_session
            .as_deref()
            .is_some_and(|s| self.tmux.session_exists(s));

        // Todo requires special handling: session liveness takes priority over
        // signal file state, because a stale signal may have been left behind.
        if *current_status == Status::Todo {
            if session_alive {
                // Agent is running — clear any stale signal and move to InProgress
                let _ = std::fs::remove_file(&signal_path);
                return Some(MonitorEvent::StatusChanged {
                    task_id: task_id.to_string(),
                    project_id: project_id.to_string(),
                    status: Status::InProgress,
                });
            } else if tmux_session.is_some() {
                // Had a session but it's dead — agent crashed or failed to start
                return Some(MonitorEvent::StatusChanged {
                    task_id: task_id.to_string(),
                    project_id: project_id.to_string(),
                    status: Status::Blocked,
                });
            }
            return None;
        }

        if signal_path.exists() {
            // Signal file present: agent stopped or idle → InReview (only from InProgress)
            if *current_status == Status::InProgress {
                return Some(MonitorEvent::StatusChanged {
                    task_id: task_id.to_string(),
                    project_id: project_id.to_string(),
                    status: Status::InReview,
                });
            }
        } else if *current_status == Status::InReview && session_alive {
            // Signal file absent + session alive: PreToolUse hook cleared signal
            return Some(MonitorEvent::StatusChanged {
                task_id: task_id.to_string(),
                project_id: project_id.to_string(),
                status: Status::InProgress,
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

