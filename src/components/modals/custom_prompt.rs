use crate::action::Action;
use crate::components::modals::Modal;
use crate::components::modals::input::TextInput;
use crate::error::AppResult;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

const MAX_CUSTOM_PROMPTS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Focus {
    List,
    Input,
}

pub struct CustomPromptModal {
    task_id: String,
    project_id: String,
    prompts: Vec<String>,
    selected: usize,
    focus: Focus,
    input: TextInput,
}

impl CustomPromptModal {
    pub fn new(task_id: String, project_id: String, prompts: Vec<String>) -> Self {
        let mut input = TextInput::new("New Prompt");
        input.focused = false;
        Self {
            task_id,
            project_id,
            prompts,
            selected: 0,
            focus: Focus::List,
            input,
        }
    }
}

impl Modal for CustomPromptModal {
    fn handle_key(&mut self, key: KeyEvent) -> AppResult<Option<Action>> {
        // Esc closes modal
        if key.code == KeyCode::Esc {
            return Ok(Some(Action::CloseModal));
        }

        // Tab switches focus between list and input
        if key.code == KeyCode::Tab || key.code == KeyCode::BackTab {
            match self.focus {
                Focus::List => {
                    self.focus = Focus::Input;
                    self.input.focused = true;
                }
                Focus::Input => {
                    self.focus = Focus::List;
                    self.input.focused = false;
                }
            }
            return Ok(None);
        }

        match self.focus {
            Focus::List => {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if self.selected > 0 {
                            self.selected -= 1;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if !self.prompts.is_empty() && self.selected < self.prompts.len() - 1 {
                            self.selected += 1;
                        }
                    }
                    // Ctrl+Enter sends selected prompt
                    KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if let Some(prompt) = self.prompts.get(self.selected) {
                            return Ok(Some(Action::SendCustomPrompt {
                                task_id: self.task_id.clone(),
                                project_id: self.project_id.clone(),
                                prompt: prompt.clone(),
                            }));
                        }
                    }
                    // Ctrl+D deletes selected prompt (also handles Delete key due to Ctrl+D remap)
                    KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if !self.prompts.is_empty() {
                            return Ok(Some(Action::DeleteCustomPrompt {
                                index: self.selected,
                            }));
                        }
                    }
                    KeyCode::Delete => {
                        if !self.prompts.is_empty() {
                            return Ok(Some(Action::DeleteCustomPrompt {
                                index: self.selected,
                            }));
                        }
                    }
                    _ => {}
                }
            }
            Focus::Input => {
                // Ctrl+Enter adds the prompt to the list
                if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL) {
                    let text = self.input.value.trim().to_string();
                    if !text.is_empty() && self.prompts.len() < MAX_CUSTOM_PROMPTS {
                        return Ok(Some(Action::AddCustomPrompt { prompt: text }));
                    }
                    return Ok(None);
                }
                // Let TextInput handle the rest
                self.input.handle_key(key);
            }
        }

        Ok(None)
    }

    fn handle_paste(&mut self, text: &str) {
        if self.focus == Focus::Input {
            self.input.insert_paste(text);
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Custom Prompt ")
            .border_style(Style::default().fg(Color::Cyan));
        frame.render_widget(block, area);

        let inner = area.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });

        // Layout: list area + hint line + input area
        let chunks = Layout::vertical([
            Constraint::Min(3),    // prompt list
            Constraint::Length(1), // hints
            Constraint::Length(3), // new prompt input
        ])
        .split(inner);

        // Render prompt list
        let list_focused = self.focus == Focus::List;
        let border_color = if list_focused {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        let lines: Vec<Line> = if self.prompts.is_empty() {
            vec![Line::from(Span::styled(
                "  (No custom prompts)",
                Style::default().fg(Color::DarkGray),
            ))]
        } else {
            self.prompts
                .iter()
                .enumerate()
                .map(|(i, prompt)| {
                    // Truncate display if too long (char-safe for multibyte)
                    let max_chars = (area.width as usize).saturating_sub(8);
                    let char_count = prompt.chars().count();
                    let display = if char_count > max_chars {
                        let truncated: String = prompt.chars().take(max_chars.saturating_sub(3)).collect();
                        format!("{}...", truncated)
                    } else {
                        prompt.clone()
                    };
                    if i == self.selected && list_focused {
                        Line::from(vec![
                            Span::styled(" > ", Style::default().fg(Color::Cyan)),
                            Span::styled(display, Style::default().fg(Color::White)),
                        ])
                    } else if i == self.selected {
                        Line::from(vec![
                            Span::styled(" > ", Style::default().fg(Color::DarkGray)),
                            Span::raw(display),
                        ])
                    } else {
                        Line::from(vec![Span::raw("   "), Span::raw(display)])
                    }
                })
                .collect()
        };

        let list_widget = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Prompts ({}/{}) ", self.prompts.len(), MAX_CUSTOM_PROMPTS))
                .border_style(Style::default().fg(border_color)),
        );
        frame.render_widget(list_widget, chunks[0]);

        // Render hints
        let hints = Line::from(vec![
            Span::styled(" C-Enter", Style::default().fg(Color::Yellow)),
            Span::raw(": Send/Add  "),
            Span::styled("C-d", Style::default().fg(Color::Yellow)),
            Span::raw(": Delete  "),
            Span::styled("Tab", Style::default().fg(Color::Yellow)),
            Span::raw(": Switch  "),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::raw(": Close"),
        ]);
        frame.render_widget(Paragraph::new(hints), chunks[1]);

        // Render input
        self.input.render(frame, chunks[2]);
    }
}

impl CustomPromptModal {
    /// Update prompts list after add/delete and adjust selection
    pub fn update_prompts(&mut self, prompts: Vec<String>) {
        self.prompts = prompts;
        if self.selected >= self.prompts.len() && !self.prompts.is_empty() {
            self.selected = self.prompts.len() - 1;
        }
        // Clear input after adding
        self.input.value.clear();
        self.input.cursor = 0;
    }
}
