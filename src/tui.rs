use crate::error::AppResult;
use crossterm::{
    execute,
    event::{
        DisableMouseCapture,
        EnableBracketedPaste, DisableBracketedPaste,
        KeyboardEnhancementFlags, PushKeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    },
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, Stdout};

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

pub fn enter() -> AppResult<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
    // Enable keyboard enhancement so terminals can distinguish Ctrl+Enter from Enter
    let _ = execute!(
        stdout,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    );
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

pub fn exit() -> AppResult<()> {
    disable_raw_mode()?;
    let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    execute!(io::stdout(), LeaveAlternateScreen, DisableBracketedPaste, DisableMouseCapture)?;
    Ok(())
}

pub fn resume() -> AppResult<Tui> {
    // Explicitly disable mouse capture before re-entering TUI.
    // tmux with `mouse on` enables mouse capture in the terminal, and this
    // state can persist after detaching. Without clearing it, normal text
    // selection (drag-to-copy) in the host terminal stops working.
    let _ = execute!(io::stdout(), DisableMouseCapture);
    enter()
}
