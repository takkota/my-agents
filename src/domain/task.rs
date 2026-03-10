use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

pub type TaskId = String;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    P1,
    P2,
    P3,
    P4,
    P5,
}

impl Priority {
    pub fn all() -> &'static [Priority] {
        &[
            Priority::P1,
            Priority::P2,
            Priority::P3,
            Priority::P4,
            Priority::P5,
        ]
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Priority::P1 => write!(f, "P1"),
            Priority::P2 => write!(f, "P2"),
            Priority::P3 => write!(f, "P3"),
            Priority::P4 => write!(f, "P4"),
            Priority::P5 => write!(f, "P5"),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Status {
    Todo,
    InProgress,
    InReview,
    Completed,
    Blocked,
}

impl Status {
    pub fn all() -> &'static [Status] {
        &[
            Status::Todo,
            Status::InProgress,
            Status::InReview,
            Status::Completed,
            Status::Blocked,
        ]
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Status::Todo => "○",
            Status::InProgress => "◉",
            Status::InReview => "◎",
            Status::Completed => "●",
            Status::Blocked => "✕",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Status::Todo => write!(f, "Todo"),
            Status::InProgress => write!(f, "In Progress"),
            Status::InReview => write!(f, "In Review"),
            Status::Completed => write!(f, "Completed"),
            Status::Blocked => write!(f, "Blocked"),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentCli {
    Claude,
    Codex,
    None,
}

impl AgentCli {
    pub fn all() -> &'static [AgentCli] {
        &[AgentCli::Claude, AgentCli::Codex, AgentCli::None]
    }

    pub fn command(&self) -> Option<&'static str> {
        match self {
            AgentCli::Claude => Some("claude"),
            AgentCli::Codex => Some("codex"),
            AgentCli::None => None,
        }
    }
}

impl fmt::Display for AgentCli {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentCli::Claude => write!(f, "Claude Code"),
            AgentCli::Codex => write!(f, "Codex"),
            AgentCli::None => write!(f, "None"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub repo_name: String,
    pub upstream_path: PathBuf,
    pub worktree_path: PathBuf,
    pub branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskLink {
    pub url: String,
    pub display_name: Option<String>,
}

impl TaskLink {
    pub fn display(&self) -> String {
        if let Some(name) = &self.display_name {
            return name.clone();
        }
        if self.url.contains("github.com") {
            let parts: Vec<&str> = self.url.trim_end_matches('/').split('/').collect();
            if parts.len() >= 2 {
                let kind = parts[parts.len() - 2];
                let num = parts[parts.len() - 1];
                if kind == "pull" || kind == "issues" {
                    let prefix = if kind == "pull" { "PR" } else { "Issue" };
                    return format!("{} #{}", prefix, num);
                }
            }
        }
        // Extract domain name without TLD (e.g., "google.com" -> "google")
        if let Some(host) = self.url
            .split("://")
            .nth(1)
            .unwrap_or(&self.url)
            .split('/')
            .next()
        {
            let host = host.trim_start_matches("www.");
            if let Some(domain) = host.split('.').next() {
                let domain = if domain.len() > 10 {
                    format!("{}...", &domain[..10])
                } else {
                    domain.to_string()
                };
                return domain;
            }
        }
        self.url.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub project_id: String,
    pub name: String,
    pub priority: Priority,
    pub status: Status,
    pub agent_cli: AgentCli,
    pub worktrees: Vec<WorktreeInfo>,
    pub links: Vec<TaskLink>,
    #[serde(default)]
    pub notes: Option<String>,
    pub tmux_session: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
