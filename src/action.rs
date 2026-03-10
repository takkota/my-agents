use crate::domain::task::{AgentCli, Priority, Status, TaskLink};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum Action {
    // Navigation
    MoveUp,
    MoveDown,
    AttachSession,

    // Modal triggers
    OpenCreateProject,
    OpenCreateTask,
    OpenEditItem,
    OpenSetLink,
    OpenSetStatus,
    OpenFilter,
    OpenSort,
    OpenConfirmDelete,
    CloseModal,
    /// Close modal and apply changes (for filter/sort)
    ApplyAndCloseModal,

    // Project CRUD
    CreateProject {
        name: String,
        repos: Vec<(String, PathBuf)>,
        worktree_copy_files: Vec<String>,
    },
    UpdateProject {
        project_id: String,
        name: String,
        repos: Vec<(String, PathBuf)>,
        worktree_copy_files: Vec<String>,
    },
    DeleteProject {
        project_id: String,
    },

    // Task CRUD
    CreateTask {
        project_id: String,
        name: String,
        priority: Priority,
        agent_cli: AgentCli,
        notes: Option<String>,
    },
    UpdateTask {
        task_id: String,
        project_id: String,
        name: String,
        priority: Priority,
        notes: Option<String>,
    },
    UpdateTaskStatus {
        task_id: String,
        project_id: String,
        status: Status,
    },
    UpdateTaskLink {
        task_id: String,
        project_id: String,
        link: TaskLink,
    },
    DeleteTask {
        project_id: String,
        task_id: String,
    },

    // Link actions
    OpenLinkInBrowser {
        url: String,
    },

    // System
    Tick,
    Quit,
}
