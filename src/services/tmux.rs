use crate::domain::task::AgentCli;
use crate::error::AppResult;
use nix::sys::termios;
use std::ffi::CString;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::path::Path;
use std::process::Command;

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
        let status = Self::tmux_cmd()
            .args([
                "new-session",
                "-d",
                "-s",
                name,
                "-c",
                &start_dir.to_string_lossy(),
            ])
            .status()?;
        if !status.success() {
            anyhow::bail!("Failed to create tmux session: {}", name);
        }

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

                let prog = CString::new("tmux").unwrap();
                let args = [
                    CString::new("tmux").unwrap(),
                    CString::new("attach-session").unwrap(),
                    CString::new("-t").unwrap(),
                    CString::new(name).unwrap(),
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
                nix::sys::wait::waitpid(
                    child,
                    Some(nix::sys::wait::WaitPidFlag::WNOHANG),
                )
                .ok();

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
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    pub fn launch_agent(&self, session: &str, cli: &AgentCli) -> AppResult<()> {
        if let Some(cmd) = cli.launch_command() {
            Self::tmux_cmd()
                .args(["send-keys", "-t", session, &cmd, "Enter"])
                .output()?;
        }
        Ok(())
    }

    pub fn session_name(project_id: &str, task_id: &str) -> String {
        format!("ma-{}-{}", project_id, &task_id[..task_id.len().min(6)])
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
                libc::read(master_fd, master_buf.as_mut_ptr() as *mut _, master_buf.len())
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
