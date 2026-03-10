use crate::domain::task::TaskLink;
use crate::services::tmux::TmuxService;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

pub struct PreviewPanel {
    content: String,
    current_session: Option<String>,
    task_links: Vec<TaskLink>,
    task_notes: Option<String>,
}

impl PreviewPanel {
    pub fn new() -> Self {
        Self {
            content: String::new(),
            current_session: None,
            task_links: Vec::new(),
            task_notes: None,
        }
    }

    pub fn update_task_info(&mut self, links: Vec<TaskLink>, notes: Option<String>) {
        self.task_links = links;
        self.task_notes = notes;
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
        !self.task_links.is_empty() || self.task_notes.is_some()
    }

    fn render_task_info(&self, frame: &mut Frame, area: Rect) {
        let mut lines: Vec<Line> = Vec::new();

        // Links section
        if !self.task_links.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(" Links: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]));
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
            if !self.task_links.is_empty() {
                lines.push(Line::from(""));
            }
            lines.push(Line::from(vec![
                Span::styled(" Notes: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(notes.as_str(), Style::default().fg(Color::White)),
            ]));
        }

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Task Info ")
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, area);
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let title = match &self.current_session {
            Some(name) => format!(" Session: {} ", name),
            None => " Preview ".to_string(),
        };

        if self.has_task_info() {
            // Calculate info panel height
            let mut info_lines: usize = 0;
            if !self.task_links.is_empty() {
                info_lines += 1 + self.task_links.len(); // header + links
            }
            if self.task_notes.is_some() {
                if !self.task_links.is_empty() {
                    info_lines += 1; // separator
                }
                info_lines += 2; // header + notes content
            }
            let info_height = (info_lines as u16 + 2).min(area.height / 3); // +2 for borders

            let chunks = Layout::vertical([
                Constraint::Length(info_height),
                Constraint::Min(3),
            ])
            .split(area);

            self.render_task_info(frame, chunks[0]);

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

            frame.render_widget(paragraph, chunks[1]);
        } else {
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
    }
}
