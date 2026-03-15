use crate::domain::task::AgentCli;
use crate::error::AppResult;
use nix::sys::termios;
use std::ffi::CString;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use unicode_width::UnicodeWidthChar;

const CTRL_Q: u8 = 0x11; // ASCII 17

pub struct TmuxService;

impl TmuxService {
    pub fn new() -> Self {
        Self
    }

    fn tmux_cmd() -> Command {
        Command::new("tmux")
    }

    pub fn is_available() -> bool {
        Command::new("tmux")
            .arg("-V")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn session_exists(&self, name: &str) -> bool {
        Self::tmux_cmd()
            .args(["has-session", "-t", name])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn create_session(&self, name: &str, start_dir: &Path) -> AppResult<()> {
        let output = Self::tmux_cmd()
            .args([
                "new-session",
                "-d",
                "-s",
                name,
                "-c",
                &start_dir.to_string_lossy(),
            ])
            .output()?;
        if !output.status.success() {
            anyhow::bail!("Failed to create tmux session: {}", name);
        }

        // Enable extended-keys so that modifier key sequences (e.g. Shift+Enter)
        // are passed through to applications inside tmux
        Self::tmux_cmd()
            .args(["set-option", "-t", name, "extended-keys", "on"])
            .output()?;

        // Add ~/.my-agents/bin to PATH so agents can use ma-task CLI
        let bin_dir = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".my-agents")
            .join("bin");
        let path_cmd = format!("export PATH=\"{}:$PATH\"", bin_dir.to_string_lossy());
        Self::tmux_cmd()
            .args(["send-keys", "-t", name, &path_cmd, "Enter"])
            .output()?;
        // Clear the export command from terminal display
        Self::tmux_cmd()
            .args(["send-keys", "-t", name, "clear", "Enter"])
            .output()?;

        Ok(())
    }

    /// Attach to a tmux session using PTY proxy with Ctrl+Q interception.
    ///
    /// Instead of using tmux bind-key (which would overwrite user config),
    /// we spawn `tmux attach-session` inside a PTY and intercept stdin at the
    /// application level. When Ctrl+Q (0x11) is detected, we kill the child
    /// process and return, effectively detaching without touching tmux keybindings.
    pub fn attach_session(&self, name: &str) -> AppResult<()> {
        let (master, slave) = open_pty()?;

        let fork_result = unsafe { nix::unistd::fork()? };

        match fork_result {
            nix::unistd::ForkResult::Child => {
                drop(master);

                nix::unistd::setsid().ok();
                unsafe {
                    libc::ioctl(slave.as_raw_fd(), libc::TIOCSCTTY.into(), 0);
                    libc::dup2(slave.as_raw_fd(), 0);
                    libc::dup2(slave.as_raw_fd(), 1);
                    libc::dup2(slave.as_raw_fd(), 2);
                }
                drop(slave);

                let prog = CString::new("tmux").expect("static string");
                let session_name = match CString::new(name) {
                    Ok(s) => s,
                    Err(_) => std::process::exit(1), // name contains null byte
                };
                let args = [
                    CString::new("tmux").expect("static string"),
                    CString::new("attach-session").expect("static string"),
                    CString::new("-t").expect("static string"),
                    session_name,
                ];
                let _ = nix::unistd::execvp(&prog, &args);
                std::process::exit(1);
            }
            nix::unistd::ForkResult::Parent { child } => {
                drop(slave);
                let result = run_attach_proxy(&master, child);

                // Use SIGKILL for instant termination — the tmux *session* is
                // unaffected because we're only killing the attach client.
                nix::sys::signal::kill(child, nix::sys::signal::Signal::SIGKILL).ok();
                // Non-blocking reap; the OS will clean up the zombie shortly if
                // not reaped here.
                nix::sys::wait::waitpid(child, Some(nix::sys::wait::WaitPidFlag::WNOHANG)).ok();

                result
            }
        }
    }

    pub fn kill_session(&self, name: &str) -> AppResult<()> {
        Self::tmux_cmd()
            .args(["kill-session", "-t", name])
            .output()?;
        Ok(())
    }

    pub fn capture_pane(&self, session: &str) -> AppResult<String> {
        let output = Self::tmux_cmd()
            .args(["capture-pane", "-t", session, "-p"])
            .output()?;
        if !output.status.success() {
            anyhow::bail!("Failed to capture tmux pane for session: {}", session);
        }
        let raw = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(sanitize_for_display(raw))
    }

    pub fn launch_agent(
        &self,
        session: &str,
        cli: &AgentCli,
        initial_prompt_file: Option<&Path>,
    ) -> AppResult<()> {
        if let Some(cmd) = cli.launch_command() {
            let full_cmd = if let Some(prompt_file) = initial_prompt_file {
                // Pass initial prompt via file using single-quoted path to prevent
                // shell injection. Any single quotes in the path are escaped.
                let escaped = prompt_file.to_string_lossy().replace('\'', "'\\''");
                format!("{} \"$(cat '{}')\"", cmd, escaped)
            } else {
                cmd
            };
            Self::tmux_cmd()
                .args(["send-keys", "-t", session, &full_cmd, "Enter"])
                .output()?;
        }
        Ok(())
    }

    pub fn send_prompt(&self, session: &str, cli: AgentCli, text: &str) -> AppResult<()> {
        match cli {
            // Codex treats rapid `send-keys ... Enter` input as a paste burst and may leave the
            // text in the composer instead of submitting it. Bracketed paste avoids that path.
            AgentCli::Codex => self.paste_text_and_submit(session, text),
            AgentCli::Claude | AgentCli::Gemini | AgentCli::None => self.send_text(session, text),
        }
    }

    fn paste_text_and_submit(&self, session: &str, text: &str) -> AppResult<()> {
        let buffer_name = format!(
            "ma-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );

        let set_status = Self::tmux_cmd()
            .args(["set-buffer", "-b", &buffer_name, "--", text])
            .status()?;
        if !set_status.success() {
            anyhow::bail!("Failed to stage tmux paste buffer for session: {}", session);
        }

        let paste_status = Self::tmux_cmd()
            .args([
                "paste-buffer",
                "-d",
                "-p",
                "-b",
                &buffer_name,
                "-t",
                session,
            ])
            .status()?;
        if !paste_status.success() {
            let _ = Self::tmux_cmd()
                .args(["delete-buffer", "-b", &buffer_name])
                .status();
            anyhow::bail!("Failed to paste text into tmux session: {}", session);
        }

        std::thread::sleep(Duration::from_millis(50));

        let enter_status = Self::tmux_cmd()
            .args(["send-keys", "-t", session, "Enter"])
            .status()?;
        if !enter_status.success() {
            anyhow::bail!("Failed to submit prompt in tmux session: {}", session);
        }

        Ok(())
    }

    pub fn send_text(&self, session: &str, text: &str) -> AppResult<()> {
        let output = Self::tmux_cmd()
            .args(["send-keys", "-t", session, text, "Enter"])
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "Failed to send text to tmux session {}: {}",
                session,
                stderr.trim()
            );
        }
        Ok(())
    }

    pub fn session_name(project_id: &str, task_id: &str) -> String {
        format!("ma-{}-{}", project_id, &task_id[..task_id.len().min(6)])
    }

    pub fn pm_session_name(project_id: &str) -> String {
        format!("ma-pm-{}", project_id)
    }

    pub fn list_sessions(&self) -> AppResult<Vec<String>> {
        let output = Self::tmux_cmd()
            .args(["list-sessions", "-F", "#{session_name}"])
            .output()?;
        if !output.status.success() {
            return Ok(vec![]);
        }
        let sessions = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect();
        Ok(sessions)
    }
}

/// Sanitize captured pane content for display in ratatui.
///
/// In CJK terminal environments, characters with East Asian Width "Ambiguous"
/// (e.g., box-drawing `─`, ellipsis `…`) are rendered as 2 cells wide by the
/// terminal but calculated as 1 cell by ratatui's `unicode-width`.  This width
/// mismatch causes the crossterm cursor position tracker to desynchronise,
/// pushing subsequent characters beyond the terminal edge where they wrap into
/// adjacent panel areas, producing garbled output.
///
/// This function replaces such characters with safe ASCII alternatives so that
/// ratatui's width calculation matches the actual terminal rendering.
fn sanitize_for_display(input: String) -> String {
    let mut output = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\n' => output.push('\n'),
            // Replace control characters (except newline) with space
            c if c.is_control() => output.push(' '),
            // Replace characters whose unicode-width (1) disagrees with typical
            // CJK terminal rendering (2).  Box-drawing (U+2500..U+257F) and a
            // few other Ambiguous-width symbols are the main offenders.
            c => {
                let uw = UnicodeWidthChar::width(c).unwrap_or(0);
                if uw == 1 && is_ambiguous_wide(c) {
                    // Emit two single-width chars to match the terminal's 2-cell rendering
                    output.push(ambiguous_replacement(c));
                    output.push(' ');
                } else {
                    output.push(c);
                }
            }
        }
    }
    output
}

/// Return true for characters that have East Asian Width "Ambiguous" and are
/// commonly rendered as 2 cells in CJK terminals.
fn is_ambiguous_wide(c: char) -> bool {
    matches!(c,
        // Box Drawing
        '\u{2500}'..='\u{257F}' |
        // Block Elements
        '\u{2580}'..='\u{259F}' |
        // Geometric Shapes
        '\u{25A0}'..='\u{25FF}' |
        // Miscellaneous Symbols
        '\u{2600}'..='\u{26FF}' |
        // Dingbats
        '\u{2700}'..='\u{27BF}' |
        // Horizontal Ellipsis
        '\u{2026}' |
        // Arrows
        '\u{2190}'..='\u{21FF}' |
        // Mathematical Operators (common in CLI output)
        '\u{2200}'..='\u{22FF}'
    )
}

/// Pick a single-width ASCII replacement for an ambiguous-width character.
fn ambiguous_replacement(c: char) -> char {
    match c {
        '\u{2500}' | '\u{2501}' | '\u{2504}' | '\u{2505}' | '\u{2508}' | '\u{2509}' => '-',
        '\u{2502}' | '\u{2503}' | '\u{2506}' | '\u{2507}' | '\u{250A}' | '\u{250B}' => '|',
        '\u{250C}'..='\u{254B}' => '+', // corners and crosses
        '\u{2026}' => '.',              // ellipsis
        _ => ' ',
    }
}

/// Open a PTY pair, returning (master, slave) as OwnedFd.
fn open_pty() -> AppResult<(OwnedFd, OwnedFd)> {
    let mut master_fd: libc::c_int = 0;
    let mut slave_fd: libc::c_int = 0;
    let ret = unsafe {
        libc::openpty(
            &mut master_fd,
            &mut slave_fd,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ret != 0 {
        anyhow::bail!("openpty failed");
    }
    let master = unsafe { OwnedFd::from_raw_fd(master_fd) };
    let slave = unsafe { OwnedFd::from_raw_fd(slave_fd) };
    Ok((master, slave))
}

/// Proxy I/O between the real terminal and the PTY master,
/// intercepting Ctrl+Q to trigger detach.
fn run_attach_proxy(master: &OwnedFd, child: nix::unistd::Pid) -> AppResult<()> {
    let stdin = io::stdin();
    let stdin_fd = stdin.as_raw_fd();
    let stdin_borrowed = unsafe { BorrowedFd::borrow_raw(stdin_fd) };
    let master_fd = master.as_raw_fd();

    // Save original terminal state and switch to raw mode
    let orig_termios = termios::tcgetattr(&stdin_borrowed)?;
    let mut raw = orig_termios.clone();
    termios::cfmakeraw(&mut raw);
    termios::tcsetattr(&stdin_borrowed, termios::SetArg::TCSANOW, &raw)?;

    // Propagate current terminal size to PTY
    copy_winsize(stdin_fd, master_fd);

    let result = io_loop(master_fd, child, stdin_fd);

    // Always restore terminal state, even if io_loop failed
    let _ = termios::tcsetattr(&stdin_borrowed, termios::SetArg::TCSANOW, &orig_termios);

    result
}

/// Main I/O loop: multiplex stdin and PTY master using poll().
fn io_loop(master_fd: i32, child: nix::unistd::Pid, stdin_fd: i32) -> AppResult<()> {
    let mut stdin_buf = [0u8; 4096];
    let mut master_buf = [0u8; 4096];
    let mut last_ws = get_winsize(stdin_fd);

    loop {
        // Check if child is still alive
        match nix::sys::wait::waitpid(child, Some(nix::sys::wait::WaitPidFlag::WNOHANG)) {
            Ok(nix::sys::wait::WaitStatus::StillAlive) => {}
            Ok(_) => break,
            Err(_) => break,
        }

        let mut poll_fds = [
            libc::pollfd {
                fd: stdin_fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: master_fd,
                events: libc::POLLIN,
                revents: 0,
            },
        ];

        let n = unsafe { libc::poll(poll_fds.as_mut_ptr(), 2, 100) };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                // EINTR likely from SIGWINCH — sync window size
                sync_winsize(stdin_fd, master_fd, &mut last_ws);
                continue;
            }
            anyhow::bail!("poll error: {}", err);
        }
        if n == 0 {
            // Timeout — check for window size changes
            sync_winsize(stdin_fd, master_fd, &mut last_ws);
            continue;
        }

        // Read from stdin, intercept Ctrl+Q
        if poll_fds[0].revents & libc::POLLIN != 0 {
            let n =
                unsafe { libc::read(stdin_fd, stdin_buf.as_mut_ptr() as *mut _, stdin_buf.len()) };
            if n <= 0 {
                break;
            }
            let n = n as usize;

            if let Some(idx) = stdin_buf[..n].iter().position(|&b| b == CTRL_Q) {
                if idx > 0 {
                    unsafe { libc::write(master_fd, stdin_buf.as_ptr() as *const _, idx) };
                }
                return Ok(());
            }

            unsafe { libc::write(master_fd, stdin_buf.as_ptr() as *const _, n) };
        }
        if poll_fds[0].revents & (libc::POLLERR | libc::POLLHUP) != 0 {
            break;
        }

        // Read from PTY master, write to stdout
        if poll_fds[1].revents & libc::POLLIN != 0 {
            let n = unsafe {
                libc::read(
                    master_fd,
                    master_buf.as_mut_ptr() as *mut _,
                    master_buf.len(),
                )
            };
            if n <= 0 {
                break;
            }
            let n = n as usize;
            let mut stdout = io::stdout().lock();
            stdout.write_all(&master_buf[..n])?;
            stdout.flush()?;
        }
        if poll_fds[1].revents & (libc::POLLERR | libc::POLLHUP) != 0 {
            break;
        }
    }

    Ok(())
}

/// Get current terminal window size.
fn get_winsize(fd: i32) -> (u16, u16) {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) == 0 {
            (ws.ws_col, ws.ws_row)
        } else {
            (0, 0)
        }
    }
}

/// Copy terminal window size from src_fd to dst_fd.
fn copy_winsize(src_fd: i32, dst_fd: i32) {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(src_fd, libc::TIOCGWINSZ, &mut ws) == 0 {
            libc::ioctl(dst_fd, libc::TIOCSWINSZ, &ws);
        }
    }
}

/// Sync window size only if it changed since last check.
fn sync_winsize(src_fd: i32, dst_fd: i32, last: &mut (u16, u16)) {
    let current = get_winsize(src_fd);
    if current != *last {
        copy_winsize(src_fd, dst_fd);
        *last = current;
    }
}
