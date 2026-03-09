use crate::services::tmux::TmuxService;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Text;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

pub struct PreviewPanel {
    content: String,
    current_session: Option<String>,
}

impl PreviewPanel {
    pub fn new() -> Self {
        Self {
            content: String::new(),
            current_session: None,
        }
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

    pub fn render(&self, frame: &mut Frame, area: Rect) {
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
}
