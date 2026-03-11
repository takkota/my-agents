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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoRef {
    pub name: String,
    pub path: PathBuf,
}
