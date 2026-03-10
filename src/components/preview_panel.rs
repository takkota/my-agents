use crate::domain::task::TaskLink;
use crate::services::tmux::TmuxService;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use std::path::PathBuf;

#[derive(Clone)]
pub struct ProjectInfo {
    pub name: String,
    pub repos: Vec<RepoInfo>,
    pub worktree_copy_files: Vec<String>,
    pub task_stats: TaskStats,
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
            task_links: Vec::new(),
            task_notes: None,
            task_initial_instructions: None,
            project_info: None,
        }
    }

    pub fn update_task_info(&mut self, links: Vec<TaskLink>, notes: Option<String>, initial_instructions: Option<String>) {
        self.task_links = links;
        self.task_notes = notes;
        self.task_initial_instructions = initial_instructions;
        self.project_info = None;
    }

    pub fn update_project_info(&mut self, info: ProjectInfo) {
        self.project_info = Some(info);
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

    fn has_task_info(&self) -> bool {
        !self.task_links.is_empty() || self.task_notes.is_some() || self.task_initial_instructions.is_some()
    }

    fn build_task_info_lines(&self) -> Vec<Line<'_>> {
        let mut lines: Vec<Line> = Vec::new();

        // Links section
        if !self.task_links.is_empty() {
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
                    Span::styled(instr_line.to_string(), Style::default().fg(Color::White)),
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
        let paragraph = Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(Color::Gray));
        frame.render_widget(paragraph, area);
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        if self.project_info.is_some() {
            self.render_project_info(frame, area);
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
