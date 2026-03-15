use crate::domain::task::{AgentCli, TaskLink};
use crate::services::tmux::TmuxService;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use std::io::{Read as _, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct ProjectInfo {
    pub name: String,
    pub description: Option<String>,
    pub project_dir: PathBuf,
    pub repos: Vec<RepoInfo>,
    pub worktree_copy_files: Vec<String>,
    pub task_stats: TaskStats,
    pub pm_enabled: bool,
    pub pm_agent_cli: Option<AgentCli>,
    pub pm_cron_expression: Option<String>,
}

#[derive(Clone)]
pub struct RepoInfo {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Clone, Default)]
pub struct TaskStats {
    pub total: usize,
    pub todo: usize,
    pub in_progress: usize,
    pub action_required: usize,
    pub completed: usize,
    pub blocked: usize,
}

pub struct PreviewPanel {
    content: String,
    current_session: Option<String>,
    task_dir: Option<PathBuf>,
    task_links: Vec<TaskLink>,
    task_notes: Option<String>,
    task_initial_instructions: Option<String>,
    project_info: Option<ProjectInfo>,
}

impl PreviewPanel {
    pub fn new() -> Self {
        Self {
            content: String::new(),
            current_session: None,
            task_dir: None,
            task_links: Vec::new(),
            task_notes: None,
            task_initial_instructions: None,
            project_info: None,
        }
    }

    pub fn update_task_info(&mut self, task_dir: PathBuf, links: Vec<TaskLink>, notes: Option<String>, initial_instructions: Option<String>) {
        self.task_dir = Some(task_dir);
        self.task_links = links;
        self.task_notes = notes;
        self.task_initial_instructions = initial_instructions;
        self.project_info = None;
    }

    pub fn clear_task_info(&mut self) {
        self.task_dir = None;
        self.task_links = Vec::new();
        self.task_notes = None;
        self.task_initial_instructions = None;
        self.project_info = None;
    }

    pub fn update_project_info(&mut self, info: ProjectInfo) {
        self.project_info = Some(info);
        self.task_dir = None;
        self.task_links = Vec::new();
        self.task_notes = None;
        self.task_initial_instructions = None;
    }

    pub fn update_preview(&mut self, session_name: Option<&str>, tmux: &TmuxService) {
        match session_name {
            Some(name) if tmux.session_exists(name) => {
                self.current_session = Some(name.to_string());
                match tmux.capture_pane(name) {
                    Ok(content) => self.content = content,
                    Err(_) => {
                        self.content = "Failed to capture session output.".to_string();
                    }
                }
            }
            _ => {
                self.current_session = None;
                self.content = "No active session.\n\nSelect a task and press Enter to start a session.".to_string();
            }
        }
    }

    /// Update preview from a file (used for PM non-interactive output).
    /// Reads only the tail of the file to avoid excessive I/O on large outputs.
    pub fn update_preview_from_file(&mut self, file_path: &Path, session_label: &str) {
        self.current_session = Some(format!("PM: {}", session_label));
        match read_file_tail(file_path, 500) {
            Ok(content) if !content.is_empty() => {
                self.content = content;
            }
            Ok(_) => {
                self.content = "PM is running... (waiting for output)".to_string();
            }
            Err(_) => {
                self.content = "No PM output yet.".to_string();
            }
        }
    }

    fn has_task_info(&self) -> bool {
        self.task_dir.is_some() || !self.task_links.is_empty() || self.task_notes.is_some() || self.task_initial_instructions.is_some()
    }

    fn build_task_info_lines(&self) -> Vec<Line<'_>> {
        let mut lines: Vec<Line> = Vec::new();

        // Directory section
        if let Some(task_dir) = &self.task_dir {
            lines.push(Line::from(vec![Span::styled(
                " Directory: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    task_dir.display().to_string(),
                    Style::default().fg(Color::White),
                ),
            ]));
        }

        // Links section
        if !self.task_links.is_empty() {
            if !lines.is_empty() {
                lines.push(Line::from(""));
            }
            lines.push(Line::from(vec![Span::styled(
                " Links: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]));
            for link in &self.task_links {
                let display = link.display();
                let hyperlink = ratatui::text::Span::styled(
                    format!(" {} ", display),
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::UNDERLINED),
                );
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    hyperlink,
                    Span::styled(
                        format!(" {}", link.url),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
        }

        // Notes section
        if let Some(notes) = &self.task_notes {
            if !lines.is_empty() {
                lines.push(Line::from(""));
            }
            lines.push(Line::from(vec![Span::styled(
                " Notes: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]));
            for note_line in notes.lines() {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(note_line.to_string(), Style::default().fg(Color::White)),
                ]));
            }
        }

        // Initial Instructions section
        if let Some(instructions) = &self.task_initial_instructions {
            if !lines.is_empty() {
                lines.push(Line::from(""));
            }
            lines.push(Line::from(vec![Span::styled(
                " Initial Instructions: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]));
            for instr_line in instructions.lines() {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        instr_line.to_string(),
                        Style::default().fg(Color::White),
                    ),
                ]));
            }
        }

        lines
    }

    fn build_task_info_paragraph(&self) -> Paragraph<'_> {
        Paragraph::new(self.build_task_info_lines())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Task Info ")
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false })
    }

    fn render_task_info(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(self.build_task_info_paragraph(), area);
    }

    fn render_project_info(&self, frame: &mut Frame, area: Rect) {
        let info = match &self.project_info {
            Some(info) => info,
            None => return,
        };

        let mut lines: Vec<Line> = Vec::new();

        // Project directory
        lines.push(Line::from(vec![Span::styled(
            " Directory ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                info.project_dir.display().to_string(),
                Style::default().fg(Color::White),
            ),
        ]));
        // Description section
        if let Some(desc) = &info.description {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    desc.clone(),
                    Style::default().fg(Color::White),
                ),
            ]));
        }

        lines.push(Line::from(""));

        // Repositories section
        lines.push(Line::from(vec![Span::styled(
            " Repositories ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]));

        if info.repos.is_empty() {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("(none)", Style::default().fg(Color::DarkGray)),
            ]));
        } else {
            for repo in &info.repos {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(&repo.name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                ]));
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(
                        repo.path.display().to_string(),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
        }

        lines.push(Line::from(""));

        // Task statistics section
        let stats = &info.task_stats;
        lines.push(Line::from(vec![Span::styled(
            " Tasks ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]));

        lines.push(Line::from(vec![
            Span::raw("  Total: "),
            Span::styled(
                stats.total.to_string(),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
        ]));

        if stats.total > 0 {
            let stat_items = [
                ("  ○ Todo:        ", stats.todo, Color::White),
                ("  ◉ In Progress: ", stats.in_progress, Color::Yellow),
                ("  ⚠ Action Req:  ", stats.action_required, Color::LightRed),
                ("  ● Completed:   ", stats.completed, Color::Green),
                ("  ✕ Blocked:     ", stats.blocked, Color::Red),
            ];
            for (label, count, color) in stat_items {
                if count > 0 {
                    lines.push(Line::from(vec![
                        Span::styled(label, Style::default().fg(color)),
                        Span::styled(
                            count.to_string(),
                            Style::default().fg(color),
                        ),
                    ]));
                }
            }
        }

        // Worktree copy files section
        if !info.worktree_copy_files.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                " Worktree Copy Files ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]));
            for file in &info.worktree_copy_files {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(file.clone(), Style::default().fg(Color::White)),
                ]));
            }
        }

        // PM section
        if info.pm_enabled {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                " Project Manager ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]));
            let agent_name = info
                .pm_agent_cli
                .map(|a| format!("{}", a))
                .unwrap_or_else(|| "Claude".to_string());
            lines.push(Line::from(vec![
                Span::raw("  Agent: "),
                Span::styled(agent_name, Style::default().fg(Color::White)),
            ]));
            if let Some(cron) = &info.pm_cron_expression {
                lines.push(Line::from(vec![
                    Span::raw("  Cron:  "),
                    Span::styled(cron.clone(), Style::default().fg(Color::White)),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" Project: {} ", info.name))
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, area);
    }

    fn render_session(&self, frame: &mut Frame, area: Rect) {
        let title = match &self.current_session {
            Some(name) => format!(" Session: {} ", name),
            None => " Preview ".to_string(),
        };
        let text = Text::from(self.content.as_str());
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::DarkGray));
        let paragraph = Paragraph::new(text)
            .block(block.clone())
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(Color::Gray));

        // Scroll to bottom so the latest tmux output is always visible
        let inner_height = block.inner(area).height as usize;
        let total_lines = paragraph.line_count(area.width);
        let scroll_offset = total_lines.saturating_sub(inner_height) as u16;
        let paragraph = paragraph.scroll((scroll_offset, 0));

        frame.render_widget(paragraph, area);
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        if self.project_info.is_some() {
            // If PM session is active, split between project info and session content
            if self.current_session.is_some() {
                let chunks = Layout::vertical([
                    Constraint::Percentage(40),
                    Constraint::Percentage(60),
                ])
                .split(area);
                self.render_project_info(frame, chunks[0]);
                self.render_session(frame, chunks[1]);
            } else {
                self.render_project_info(frame, area);
            }
            return;
        }

        if self.has_task_info() {
            // Use Paragraph::line_count() to get accurate height including word-wrapping
            let paragraph = self.build_task_info_paragraph();
            let info_lines = paragraph.line_count(area.width) as u16;
            let max_height = if self.current_session.is_some() {
                area.height / 3
            } else {
                area.height * 2 / 3
            };
            let info_height = info_lines.min(max_height);

            let chunks = Layout::vertical([
                Constraint::Length(info_height),
                Constraint::Min(3),
            ])
            .split(area);

            self.render_task_info(frame, chunks[0]);
            self.render_session(frame, chunks[1]);
        } else {
            self.render_session(frame, area);
        }
    }
}

/// Read the last `max_lines` lines from a file efficiently by seeking from the end.
/// Falls back to full read for small files (< 64KB).
fn read_file_tail(path: &Path, max_lines: usize) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    let file_size = metadata.len();

    // For small files, just read everything
    const TAIL_BUF_SIZE: u64 = 64 * 1024; // 64KB
    if file_size <= TAIL_BUF_SIZE {
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        return Ok(content);
    }

    // Seek to near the end and read the tail chunk
    file.seek(SeekFrom::End(-(TAIL_BUF_SIZE as i64)))?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;

    // Skip the first partial line (we likely landed mid-line)
    let lines: Vec<&str> = buf.lines().collect();
    let start = if lines.len() > max_lines {
        lines.len() - max_lines
    } else {
        1 // skip first partial line
    };
    Ok(lines[start..].join("\n"))
}
