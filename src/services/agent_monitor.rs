use crate::domain::task::{AgentCli, Status};
use crate::services::tmux::TmuxService;
use crate::storage::FsStore;

pub struct AgentMonitor {
    store: FsStore,
    tmux: TmuxService,
}

pub enum MonitorEvent {
    StatusChanged { task_id: String, status: Status },
}

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
            let session_name = match &task.tmux_session {
                Some(s) => s,
                None => continue,
            };
            if !self.tmux.session_exists(session_name) {
                continue;
            }

            let content = match self.tmux.capture_pane(session_name) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let is_waiting = is_waiting_for_input(&content, &task.agent_cli);

            match (is_waiting, &task.status) {
                (true, Status::InProgress) => {
                    events.push(MonitorEvent::StatusChanged {
                        task_id: task.id.clone(),
                        status: Status::InReview,
                    });
                }
                (false, Status::InReview) => {
                    events.push(MonitorEvent::StatusChanged {
                        task_id: task.id.clone(),
                        status: Status::InProgress,
                    });
                }
                _ => {}
            }
        }

        events
    }
}

fn is_waiting_for_input(content: &str, cli: &AgentCli) -> bool {
    let last_lines: String = content
        .lines()
        .rev()
        .take(5)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");

    match cli {
        AgentCli::Claude => {
            last_lines.contains("❯")
                || last_lines.contains("> ")
                || last_lines.contains("Do you want")
                || last_lines.contains("(y/n)")
        }
        AgentCli::Codex => {
            last_lines.contains("Approve?")
                || last_lines.contains("(y/n)")
                || last_lines.contains("> ")
        }
        AgentCli::None => false,
    }
}
