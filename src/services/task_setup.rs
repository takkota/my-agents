use crate::domain::project::Project;
use crate::domain::task::{AgentCli, Task, WorktreeInfo};
use crate::services::tmux::TmuxService;
use crate::services::worktree::WorktreeService;
use crate::storage::FsStore;
use std::path::Path;

/// Input for the task setup pipeline.
pub struct TaskSetupInput<'a> {
    pub task: &'a Task,
    pub project: &'a Project,
    pub task_dir: &'a Path,
    pub pr_prompt: String,
}

/// Output from the task setup pipeline.
pub struct TaskSetupOutput {
    pub worktrees: Vec<WorktreeInfo>,
    pub tmux_session: Option<String>,
    pub error: Option<String>,
}

/// Run the full task setup pipeline synchronously:
/// worktree creation → initial prompt file → agent config files → tmux session → agent launch.
///
/// This function never panics. Errors are accumulated in `TaskSetupOutput::error`.
pub fn run_task_setup(
    input: TaskSetupInput<'_>,
    store: &FsStore,
    tmux: &TmuxService,
) -> TaskSetupOutput {
    let mut error_msg: Option<String> = None;

    let repos: Vec<(String, std::path::PathBuf)> = input
        .project
        .repos
        .iter()
        .map(|r| (r.name.clone(), r.path.clone()))
        .collect();

    // Create worktrees (3-phase: create without checkout → copy files → checkout)
    // This ordering ensures that copied files (e.g. .env) are in place before
    // post-checkout hooks run, so hooks can update them rather than having
    // the copy overwrite hook-generated files.
    let worktree_svc = WorktreeService::new();
    let worktrees = if !repos.is_empty() {
        match worktree_svc.create_worktrees_for_task(input.task_dir, &input.task.id, &repos) {
            Ok(wts) => {
                // Phase 2: Copy files into worktrees (before checkout)
                if !input.project.worktree_copy_files.is_empty() {
                    for wt in &wts {
                        if let Err(e) = WorktreeService::copy_files_to_worktree(
                            &wt.upstream_path,
                            &wt.worktree_path,
                            &input.project.worktree_copy_files,
                        ) {
                            append_error(
                                &mut error_msg,
                                &format!("Failed to copy files to worktree {}: {}", wt.repo_name, e),
                            );
                        }
                    }
                }
                // Phase 3: Checkout (triggers post-checkout hooks)
                // If checkout fails, remove the broken worktree and exclude it.
                let mut checked_out = Vec::new();
                for wt in wts {
                    if let Err(e) = WorktreeService::checkout_worktree(&wt.worktree_path, &wt.branch) {
                        append_error(
                            &mut error_msg,
                            &format!("Worktree checkout failed for {}: {}", wt.repo_name, e),
                        );
                        let _ = worktree_svc.remove_worktree(&wt);
                    } else {
                        checked_out.push(wt);
                    }
                }
                checked_out
            }
            Err(e) => {
                append_error(&mut error_msg, &format!("Worktree creation failed: {}", e));
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    // Build initial prompt file if instructions were provided
    let prompt_file = if let Some(instructions) = &input.task.initial_instructions {
        if input.task.agent_cli != AgentCli::None {
            let mut prompt = instructions.clone();
            let link_urls: Vec<String> = input.task.links.iter().map(|l| l.url.clone()).collect();
            if !link_urls.is_empty() {
                prompt.push_str("\n\nLinks:\n");
                for url in &link_urls {
                    prompt.push_str(&format!("- {}\n", url));
                }
            }
            let path = input.task_dir.join(".initial_prompt");
            match std::fs::write(&path, &prompt) {
                Ok(()) => Some(path),
                Err(e) => {
                    append_error(
                        &mut error_msg,
                        &format!("Failed to write initial prompt file: {}", e),
                    );
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    // Write agent config files BEFORE launching the agent so that
    // hooks (settings.json) are in place when the agent starts.
    let mut updated_task = input.task.clone();
    updated_task.worktrees = worktrees.clone();
    if let Err(e) = store
        .save_task(&updated_task)
        .and_then(|_| store.write_agent_config_files(&updated_task, &input.pr_prompt))
    {
        append_error(&mut error_msg, &format!("{}", e));
    }

    // Create tmux session
    let session_name = TmuxService::session_name(&updated_task.project_id, &updated_task.id);
    let tmux_session = if TmuxService::is_available() {
        match tmux.create_session(&session_name, input.task_dir) {
            Ok(()) => {
                if updated_task.agent_cli != AgentCli::None {
                    match tmux.launch_agent(
                        &session_name,
                        &updated_task.agent_cli,
                        prompt_file.as_deref(),
                    ) {
                        Ok(()) => {
                            // If an initial prompt was provided, create the
                            // .prompt_submitted marker so the monitor can
                            // transition Todo → InProgress even if the
                            // UserPromptSubmit hook doesn't fire for piped prompts.
                            if prompt_file.is_some() {
                                let _ = std::fs::write(input.task_dir.join(".prompt_submitted"), "");
                            }
                        }
                        Err(e) => {
                            append_error(&mut error_msg, &format!("Agent launch failed: {}", e));
                        }
                    }
                }
                Some(session_name)
            }
            Err(e) => {
                append_error(&mut error_msg, &format!("tmux session creation failed: {}", e));
                None
            }
        }
    } else {
        None
    };

    // Update task with tmux session info
    updated_task.tmux_session = tmux_session.clone();
    if let Err(e) = store.save_task(&updated_task) {
        append_error(&mut error_msg, &format!("{}", e));
    }

    TaskSetupOutput {
        worktrees,
        tmux_session,
        error: error_msg,
    }
}

fn append_error(error_msg: &mut Option<String>, msg: &str) {
    match error_msg {
        Some(existing) => {
            existing.push_str("; ");
            existing.push_str(msg);
        }
        None => {
            *error_msg = Some(msg.to_string());
        }
    }
}
