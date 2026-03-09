use super::Modal;
use crate::action::Action;
use crate::error::AppResult;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

pub enum DeleteTarget {
    Project { id: String, name: String },
    Task { project_id: String, task_id: String, name: String },
}

pub struct ConfirmDeleteModal {
    target: DeleteTarget,
}

impl ConfirmDeleteModal {
    pub fn new(target: DeleteTarget) -> Self {
        Self { target }
    }
}

impl Modal for ConfirmDeleteModal {
    fn handle_key(&mut self, key: KeyEvent) -> AppResult<Option<Action>> {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let action = match &self.target {
                    DeleteTarget::Project { id, .. } => Action::DeleteProject {
                        project_id: id.clone(),
                    },
                    DeleteTarget::Task { project_id, task_id, .. } => Action::DeleteTask {
                        project_id: project_id.clone(),
                        task_id: task_id.clone(),
                    },
                };
                Ok(Some(action))
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                Ok(Some(Action::CloseModal))
            }
            _ => Ok(None),
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);

        let name = match &self.target {
            DeleteTarget::Project { name, .. } => format!("project \"{}\"", name),
            DeleteTarget::Task { name, .. } => format!("task \"{}\"", name),
        };

        let text = vec![
            Line::from(""),
            Line::from(vec![
                Span::raw("  Delete "),
                Span::styled(&name, Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw("?"),
            ]),
            Line::from(""),
            Line::from("  This will remove all associated resources"),
            Line::from("  (worktrees, tmux sessions, directories)."),
            Line::from(""),
            Line::from(vec![
                Span::raw("  Press "),
                Span::styled("y", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw(" to confirm, "),
                Span::styled("n", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw(" to cancel"),
            ]),
        ];

        let paragraph = Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Confirm Delete ")
                .border_style(Style::default().fg(Color::Red)),
        );
        frame.render_widget(paragraph, area);
    }

    fn title(&self) -> &str {
        "Confirm Delete"
    }
}
