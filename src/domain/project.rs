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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoRef {
    pub name: String,
    pub path: PathBuf,
}
