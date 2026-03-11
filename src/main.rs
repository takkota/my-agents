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

#[tokio::main]
async fn main() -> AppResult<()> {
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
                            if let Err(e) = app.tmux.attach_session(&session_name) {
                                app.error_message = Some(format!("tmux attach failed: {}", e));
                            }
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
