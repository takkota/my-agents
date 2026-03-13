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
    ActionRequired,
    Completed,
    Blocked,
}

impl Status {
    pub fn all() -> &'static [Status] {
        &[
            Status::Todo,
            Status::InProgress,
            Status::ActionRequired,
            Status::Completed,
            Status::Blocked,
        ]
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Status::Todo => "☐",
            Status::InProgress => "▶",
            Status::ActionRequired => "⚠",
            Status::Completed => "✓",
            Status::Blocked => "✕",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Status::Todo => write!(f, "Todo"),
            Status::InProgress => write!(f, "In Progress"),
            Status::ActionRequired => write!(f, "Action Required"),
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

    /// Returns the full launch command with CLI-specific flags.
    pub fn launch_command(&self) -> Option<String> {
        self.command().map(|cmd| match self {
            AgentCli::Claude => format!("{} --enable-auto-mode", cmd),
            _ => cmd.to_string(),
        })
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewUrl {
    pub service_name: String,
    pub url: String,
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
        // Extract second-level domain name (e.g., "sub.example.com" -> "example", "example.com" -> "example")
        if let Some(host) = self.url
            .split("://")
            .nth(1)
            .unwrap_or(&self.url)
            .split('/')
            .next()
        {
            let host = host.trim_start_matches("www.");
            let parts: Vec<&str> = host.split('.').collect();
            let domain_part = if parts.len() >= 3 {
                // Has subdomain(s): take second-to-last part (e.g., "lcl-bus.myjetbrains.com" -> "myjetbrains")
                parts[parts.len() - 2]
            } else {
                // No subdomain: take first part (e.g., "example.com" -> "example")
                parts[0]
            };
            let domain = if domain_part.len() > 15 {
                format!("{}...", &domain_part[..15])
            } else {
                domain_part.to_string()
            };
            return domain;
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
    pub preview_urls: Vec<PreviewUrl>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub initial_instructions: Option<String>,
    pub tmux_session: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Set when a Completed task is reopened (transitioned back to InProgress).
    /// PrMonitor uses this to avoid auto-completing from already-merged PRs.
    #[serde(default)]
    pub reopened_at: Option<DateTime<Utc>>,
}
