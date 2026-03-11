use super::input::TextInput;
use super::{centered_rect_with_max, Modal};
use crate::action::Action;
use crate::config::Config;
use crate::error::AppResult;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

enum Field {
    PrPrompt,
    ReviewPrompt,
}

pub struct SettingsModal {
    pr_prompt: TextInput,
    review_prompt: TextInput,
    focused_field: Field,
}

impl SettingsModal {
    pub fn new(config: &Config) -> Self {
        let mut pr_prompt =
            TextInput::new("PR Prompt (Shift+P)").with_value(&config.pr_prompt);
        pr_prompt.focused = true;
        let review_prompt =
            TextInput::new("Review Prompt (Shift+R)").with_value(&config.review_prompt);

        Self {
            pr_prompt,
            review_prompt,
            focused_field: Field::PrPrompt,
        }
    }

    fn focus_next(&mut self) {
        match self.focused_field {
            Field::PrPrompt => {
                self.focused_field = Field::ReviewPrompt;
                self.pr_prompt.focused = false;
                self.review_prompt.focused = true;
            }
            Field::ReviewPrompt => {
                self.focused_field = Field::PrPrompt;
                self.review_prompt.focused = false;
                self.pr_prompt.focused = true;
            }
        }
    }

    fn focus_prev(&mut self) {
        self.focus_next(); // Only 2 fields, prev == next
    }

    fn focused_input(&mut self) -> &mut TextInput {
        match self.focused_field {
            Field::PrPrompt => &mut self.pr_prompt,
            Field::ReviewPrompt => &mut self.review_prompt,
        }
    }
}

impl Modal for SettingsModal {
    fn handle_key(&mut self, key: KeyEvent) -> AppResult<Option<Action>> {
        match key.code {
            KeyCode::Esc => return Ok(Some(Action::CloseModal)),
            KeyCode::Tab => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.focus_prev();
                } else {
                    self.focus_next();
                }
                return Ok(None);
            }
            KeyCode::BackTab => {
                self.focus_prev();
                return Ok(None);
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(Some(Action::SaveSettings {
                    pr_prompt: self.pr_prompt.value.clone(),
                    review_prompt: self.review_prompt.value.clone(),
                }));
            }
            _ => {}
        }

        self.focused_input().handle_key(key);
        Ok(None)
    }

    fn handle_paste(&mut self, text: &str) {
        self.focused_input().insert_paste(text);
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let modal_area = centered_rect_with_max(80, 60, 100, 20, area);
        frame.render_widget(Clear, modal_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Settings ")
            .border_style(Style::default().fg(Color::Cyan));
        frame.render_widget(block, modal_area);

        let inner = modal_area.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });

        let chunks = Layout::vertical([
            Constraint::Length(3), // PR prompt
            Constraint::Length(3), // Review prompt
            Constraint::Length(1), // Help text
        ])
        .split(inner);

        self.pr_prompt.render(frame, chunks[0]);
        self.review_prompt.render(frame, chunks[1]);

        let help = Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" Switch field  ", Style::default().fg(Color::Gray)),
            Span::styled("Ctrl+Enter", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" Save  ", Style::default().fg(Color::Gray)),
            Span::styled("Esc", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" Cancel", Style::default().fg(Color::Gray)),
        ]);
        frame.render_widget(Paragraph::new(help), chunks[2]);
    }
}
