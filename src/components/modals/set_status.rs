use super::input::SelectList;
use super::Modal;
use crate::action::Action;
use crate::domain::task::Status;
use crate::error::AppResult;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear};
use ratatui::Frame;

pub struct SetStatusModal {
    task_id: String,
    project_id: String,
    list: SelectList<Status>,
}

impl SetStatusModal {
    pub fn new(task_id: String, project_id: String, current: Status) -> Self {
        let items: Vec<(String, Status)> = Status::all()
            .iter()
            .map(|s| (format!("{} {}", s.symbol(), s), *s))
            .collect();
        let mut list = SelectList::new("Status", items);
        list.selected = Status::all()
            .iter()
            .position(|s| *s == current)
            .unwrap_or(0);
        list.focused = true;

        Self {
            task_id,
            project_id,
            list,
        }
    }
}

impl Modal for SetStatusModal {
    fn handle_key(&mut self, key: KeyEvent) -> AppResult<Option<Action>> {
        match key.code {
            KeyCode::Esc => Ok(Some(Action::CloseModal)),
            KeyCode::Up | KeyCode::Char('k') => {
                self.list.move_up();
                Ok(None)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.list.move_down();
                Ok(None)
            }
            KeyCode::Enter => {
                if let Some(status) = self.list.selected_value().copied() {
                    Ok(Some(Action::UpdateTaskStatus {
                        task_id: self.task_id.clone(),
                        project_id: self.project_id.clone(),
                        status,
                    }))
                } else {
                    Ok(Some(Action::CloseModal))
                }
            }
            _ => Ok(None),
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Set Status ")
            .border_style(Style::default().fg(Color::Cyan));
        frame.render_widget(block.clone(), area);

        let inner = area.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });
        self.list.render(frame, inner);
    }

    fn title(&self) -> &str {
        "Set Status"
    }
}
