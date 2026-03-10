use crate::domain::task::{AgentCli, Priority, Status, TaskLink};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum Action {
    // Navigation
    MoveUp,
    MoveDown,
    ToggleExpand,
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
    },
    UpdateProject {
        project_id: String,
        name: String,
        repos: Vec<(String, PathBuf)>,
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
    },
    UpdateTaskName {
        task_id: String,
        project_id: String,
        name: String,
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

    // Background monitor
    AgentStatusChanged {
        task_id: String,
        project_id: String,
        status: Status,
    },
    /// All PRs linked to this task have been merged
    AllPrsMerged {
        task_id: String,
        project_id: String,
    },

    // System
    Tick,
    Quit,
    Noop,
}
