mod action;
mod app;
mod components;
mod config;
mod domain;
mod error;
mod event;
mod services;
mod storage;
mod tui;

use app::{App, UpdateResult};
use config::Config;
use error::AppResult;
use event::{Event, EventHandler};
use services::task_setup::{self, write_initial_prompt, TaskSetupInput};
use services::tmux::TmuxService;
use storage::FsStore;

fn check_dependencies() {
    use std::process::Command;

    let missing: Vec<&str> = ["tmux", "git"]
        .into_iter()
        .filter(|cmd| {
            Command::new(cmd)
                .arg("--version")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_err()
        })
        .collect();

    if !missing.is_empty() {
        eprintln!("Error: Required dependencies not found: {}", missing.join(", "));
        eprintln!();
        for cmd in &missing {
            match *cmd {
                "tmux" => {
                    eprintln!("Install tmux:");
                    eprintln!("  brew install tmux       # macOS");
                    eprintln!("  sudo apt install tmux   # Ubuntu/Debian");
                    eprintln!("  sudo dnf install tmux   # Fedora");
                    eprintln!("  sudo pacman -S tmux     # Arch Linux");
                }
                "git" => {
                    eprintln!("Install git:");
                    eprintln!("  brew install git        # macOS");
                    eprintln!("  sudo apt install git    # Ubuntu/Debian");
                }
                _ => {}
            }
            eprintln!();
        }
        std::process::exit(1);
    }
}

fn cmd_setup_task(args: &[String]) -> AppResult<()> {
    let mut project_id = None;
    let mut task_id = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--project" | "-p" => {
                project_id = args.get(i + 1).cloned();
                i += 2;
            }
            "--task" | "-t" => {
                task_id = args.get(i + 1).cloned();
                i += 2;
            }
            "--help" | "-h" => {
                eprintln!("Usage: my-agents setup-task --project <id> --task <id>");
                eprintln!();
                eprintln!("Set up worktree, agent config files, tmux session, and launch agent for an existing task.");
                std::process::exit(0);
            }
            other => {
                anyhow::bail!("Unknown option: {}. Usage: my-agents setup-task --project <id> --task <id>", other);
            }
        }
    }

    let project_id = project_id.ok_or_else(|| anyhow::anyhow!("--project is required"))?;
    let task_id = task_id.ok_or_else(|| anyhow::anyhow!("--task is required"))?;

    // Check required dependencies for setup-task
    if !TmuxService::is_available() {
        anyhow::bail!("tmux is required for setup-task but not found in PATH");
    }

    let config = Config::load()?;
    let store = FsStore::new(&config)?;

    let project = store
        .list_projects()?
        .into_iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| anyhow::anyhow!("Project not found: {}", project_id))?;

    let task = store
        .list_tasks(&project_id)?
        .into_iter()
        .find(|t| t.id == task_id)
        .ok_or_else(|| anyhow::anyhow!("Task not found: {}", task_id))?;

    // initial_instructions is required to run a task
    match &task.initial_instructions {
        Some(s) if !s.trim().is_empty() => {}
        _ => {
            anyhow::bail!(
                "Task {} has no initial_instructions. Set them with 'ma-task update {} --prompt <text>' before running.",
                task_id, task_id
            );
        }
    }

    // Guard against re-running setup on a task that already has worktrees or a session
    if !task.worktrees.is_empty() || task.tmux_session.is_some() {
        anyhow::bail!(
            "Task {} already has worktrees or a tmux session. setup-task is intended for newly created tasks only.",
            task_id
        );
    }

    let task_dir = store.task_dir(&project_id, &task_id);

    let tmux = TmuxService::new();
    let output = task_setup::run_task_setup(
        TaskSetupInput {
            task: &task,
            project: &project,
            task_dir: &task_dir,
            pr_prompt: config.pr_prompt.clone(),
        },
        &store,
        &tmux,
    );

    // Print result as JSON
    let worktree_paths: Vec<String> = output
        .worktrees
        .iter()
        .map(|w| w.worktree_path.display().to_string())
        .collect();
    let result = serde_json::json!({
        "task_id": task_id,
        "project_id": project_id,
        "tmux_session": output.tmux_session,
        "worktrees": worktree_paths,
    });
    println!("{}", serde_json::to_string_pretty(&result)?);

    if let Some(error) = output.error {
        eprintln!("Warning: {}", error);
        std::process::exit(1);
    }

    Ok(())
}

/// Launch an agent in an existing tmux session for a task that already has
/// worktrees/session set up but no agent running yet.
fn cmd_launch_agent(args: &[String]) -> AppResult<()> {
    let mut project_id = None;
    let mut task_id = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--project" | "-p" => {
                project_id = args.get(i + 1).cloned();
                i += 2;
            }
            "--task" | "-t" => {
                task_id = args.get(i + 1).cloned();
                i += 2;
            }
            "--help" | "-h" => {
                eprintln!("Usage: my-agents launch-agent --project <id> --task <id>");
                eprintln!();
                eprintln!("Launch agent in an existing tmux session for a task that already has worktrees/session.");
                std::process::exit(0);
            }
            other => {
                anyhow::bail!("Unknown option: {}. Usage: my-agents launch-agent --project <id> --task <id>", other);
            }
        }
    }

    let project_id = project_id.ok_or_else(|| anyhow::anyhow!("--project is required"))?;
    let task_id = task_id.ok_or_else(|| anyhow::anyhow!("--task is required"))?;

    if !TmuxService::is_available() {
        anyhow::bail!("tmux is required for launch-agent but not found in PATH");
    }

    let config = Config::load()?;
    let store = FsStore::new(&config)?;

    let project = store
        .list_projects()?
        .into_iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| anyhow::anyhow!("Project not found: {}", project_id))?;

    let task = store
        .list_tasks(&project_id)?
        .into_iter()
        .find(|t| t.id == task_id)
        .ok_or_else(|| anyhow::anyhow!("Task not found: {}", task_id))?;

    if task.agent_cli == domain::task::AgentCli::None {
        anyhow::bail!("Task {} has no agent CLI configured", task_id);
    }

    // initial_instructions is required to launch an agent
    match &task.initial_instructions {
        Some(s) if !s.trim().is_empty() => {}
        _ => {
            anyhow::bail!(
                "Task {} has no initial_instructions. Set them with 'ma-task update {} --prompt <text>' before running.",
                task_id, task_id
            );
        }
    }

    let session_name = task
        .tmux_session
        .clone()
        .unwrap_or_else(|| TmuxService::session_name(&project_id, &task_id));

    let tmux = TmuxService::new();
    let task_dir = store.task_dir(&project_id, &task_id);

    // Recreate tmux session if it was lost (e.g. after reboot or manual kill)
    if !tmux.session_exists(&session_name) {
        if !task_dir.exists() {
            anyhow::bail!(
                "Task directory '{}' does not exist. Cannot recreate session.",
                task_dir.display()
            );
        }
        tmux.create_session(&session_name, &task_dir)?;
    }

    // Build initial prompt file from task's initial_instructions + links
    let prompt_file = write_initial_prompt(&task, &task_dir)?;

    // Ensure agent config files are in place
    if let Err(e) = store.write_agent_config_files(&task, &config.pr_prompt, Some(&project)) {
        eprintln!("Warning: Failed to write agent config files: {}", e);
    }

    // Launch agent
    tmux.launch_agent(&session_name, &task.agent_cli, prompt_file.as_deref())?;

    // Create .prompt_submitted marker if prompt was provided
    if prompt_file.is_some() {
        let _ = std::fs::write(task_dir.join(".prompt_submitted"), "");
    }

    // Persist agent_launched flag
    let mut task = task;
    task.agent_launched = true;
    task.tmux_session = Some(session_name.clone());
    store.save_task(&task)?;

    let result = serde_json::json!({
        "task_id": task_id,
        "project_id": project_id,
        "tmux_session": session_name,
        "agent_launched": true,
    });
    println!("{}", serde_json::to_string_pretty(&result)?);

    Ok(())
}

#[tokio::main]
async fn main() -> AppResult<()> {
    // Dispatch subcommands before TUI initialization
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("setup-task") => return cmd_setup_task(&args[2..]),
        Some("launch-agent") => return cmd_launch_agent(&args[2..]),
        _ => {}
    }

    // Check required external dependencies
    check_dependencies();

    // Install panic hook to restore terminal
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = tui::exit();
        original_hook(info);
    }));

    let config = Config::load()?;
    let mut app = App::new(config.clone())?;
    let mut terminal = tui::enter()?;
    let mut events = EventHandler::new(config.tick_rate_ms);

    loop {
        // If a full redraw was requested (e.g. after background threads that may
        // have written to the terminal), clear ratatui's front buffer so the next
        // draw overwrites every cell.
        if app.needs_full_redraw {
            terminal.clear()?;
            app.needs_full_redraw = false;
        }

        // Render
        terminal.draw(|frame| app.render(frame))?;

        // Handle events
        let event = events.next().await?;

        match event {
            Event::Key(key) => {
                if let Some(action) = app.handle_key_event(key)? {
                    match app.update(action) {
                        Ok(UpdateResult::Continue) => {}
                        Ok(UpdateResult::AttachSession(session_name)) => {
                            // Drop event handler to release stdin before tmux attach.
                            // Drop is enough — it aborts the background task immediately.
                            drop(events);
                            tui::exit()?;

                            // Spawn a background thread that continues monitoring task
                            // status while the user is inside the tmux session.
                            let bg_store = app.store.clone();
                            let bg_tmux = app.tmux.clone();
                            let bg_interval = config.monitor_interval_secs.max(1);
                            let stop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                            let stop_flag_bg = stop_flag.clone();
                            let bg_handle = std::thread::spawn(move || {
                                while !stop_flag_bg.load(std::sync::atomic::Ordering::Relaxed) {
                                    services::agent_monitor::run_monitor_cycle(&bg_store, &bg_tmux);
                                    // Sleep in small increments so we can stop promptly
                                    for _ in 0..(bg_interval * 4) {
                                        if stop_flag_bg.load(std::sync::atomic::Ordering::Relaxed) {
                                            return;
                                        }
                                        std::thread::sleep(std::time::Duration::from_millis(250));
                                    }
                                }
                            });

                            if let Err(e) = app.tmux.attach_session(&session_name) {
                                app.error_message = Some(format!("tmux attach failed: {}", e));
                            }

                            // Stop background monitor and wait for it to finish
                            stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                            let _ = bg_handle.join();

                            // Resume TUI and event handler first for instant visual feedback,
                            // then reload data in background.
                            terminal = tui::resume()?;
                            events = EventHandler::new(config.tick_rate_ms);
                            app.needs_full_redraw = true;
                            app.reload_data()?;
                        }
                        Err(e) => {
                            app.error_message = Some(format!("{}", e));
                        }
                    }
                }
            }
            Event::Paste(text) => {
                app.handle_paste_event(&text);
            }
            Event::Tick => {
                if let Err(e) = app.update(action::Action::Tick) {
                    app.error_message = Some(format!("{}", e));
                }
            }
        }

        if !app.running {
            break;
        }
    }

    tui::exit()?;
    Ok(())
}
