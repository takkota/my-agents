use crate::domain::task::AgentCli;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub repos: Vec<RepoRef>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub worktree_copy_files: Vec<String>,
    #[serde(default)]
    pub dev_environment_prompt: Option<String>,
    #[serde(default)]
    pub pm_enabled: bool,
    #[serde(default)]
    pub pm_agent_cli: Option<AgentCli>,
    #[serde(default)]
    pub pm_custom_instructions: Option<String>,
    #[serde(default)]
    pub pm_cron_expression: Option<String>,
    #[serde(default)]
    pub pm_tmux_session: Option<String>,
    // GitHub Issue monitor settings (per-project).
    // When enabled, the app polls the monitored repositories for issues
    // matching the labels and auto-creates a task per matching issue.
    #[serde(default)]
    pub issue_monitor_enabled: bool,
    /// Subset of `repos` (by RepoRef.name) to watch.
    #[serde(default)]
    pub issue_monitor_repos: Vec<String>,
    /// Labels an issue must have ALL of (AND) to be picked up.
    #[serde(default)]
    pub issue_monitor_labels: Vec<String>,
    /// Agent CLI to launch for auto-generated tasks. Defaults to the global
    /// `Config::default_agent_cli` when None.
    #[serde(default)]
    pub issue_monitor_agent_cli: Option<AgentCli>,
    /// Base prompt prepended to the issue URL for auto-generated tasks.
    #[serde(default)]
    pub issue_monitor_initial_prompt: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoRef {
    pub name: String,
    pub path: PathBuf,
}
