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
    FilterActionRequired,
    OpenSort,
    OpenConfirmDelete,
    OpenSettings,
    CloseModal,
    /// Close modal and apply changes (for filter/sort)
    ApplyAndCloseModal,

    // Project CRUD
    CreateProject {
        name: String,
        description: Option<String>,
        repos: Vec<(String, PathBuf)>,
        worktree_copy_files: Vec<String>,
    },
    UpdateProject {
        project_id: String,
        name: String,
        description: Option<String>,
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
        links: Vec<TaskLink>,
        initial_instructions: Option<String>,
    },
    UpdateTask {
        task_id: String,
        project_id: String,
        name: String,
        priority: Priority,
        notes: Option<String>,
    },
    UpdateTaskPriority {
        task_id: String,
        project_id: String,
        priority: Priority,
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

    // Settings
    SaveSettings {
        pr_prompt: String,
        review_prompt: String,
    },

    // Agent instructions
    SendReviewInstruction {
        task_id: String,
        project_id: String,
    },
    SendPrInstruction {
        task_id: String,
        project_id: String,
    },

    // System
    Tick,
    Quit,
}
