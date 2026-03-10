use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Simple single-line text input field (UTF-8 safe, cursor tracks char indices)
#[derive(Debug, Clone)]
pub struct TextInput {
    pub value: String,
    /// Cursor position as character index (not byte index)
    pub cursor: usize,
    pub label: String,
    pub focused: bool,
}

impl TextInput {
    pub fn new(label: &str) -> Self {
        Self {
            value: String::new(),
            cursor: 0,
            label: label.to_string(),
            focused: false,
        }
    }

    pub fn with_value(mut self, value: &str) -> Self {
        self.value = value.to_string();
        self.cursor = value.chars().count();
        self
    }

    fn byte_offset(&self) -> usize {
        self.value
            .char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.value.len())
    }

    pub fn insert_char(&mut self, c: char) {
        let byte_pos = self.byte_offset();
        self.value.insert(byte_pos, c);
        self.cursor += 1;
    }

    /// Insert a string (e.g. from paste), converting newlines to spaces
    pub fn insert_paste(&mut self, text: &str) {
        let sanitized = text.replace("\r\n", " ").replace(['\n', '\r'], " ");
        for c in sanitized.chars() {
            self.insert_char(c);
        }
    }

    pub fn delete_char(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let byte_pos = self.byte_offset();
            self.value.remove(byte_pos);
        }
    }

    pub fn delete_forward_char(&mut self) {
        if self.cursor < self.value.chars().count() {
            let byte_pos = self.byte_offset();
            self.value.remove(byte_pos);
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.value.chars().count() {
            self.cursor += 1;
        }
    }

    pub fn move_to_start(&mut self) {
        self.cursor = 0;
    }

    pub fn move_to_end(&mut self) {
        self.cursor = self.value.chars().count();
    }

    pub fn delete_to_start(&mut self) {
        if self.cursor > 0 {
            let byte_pos = self.byte_offset();
            self.value.drain(..byte_pos);
            self.cursor = 0;
        }
    }

    pub fn delete_to_end(&mut self) {
        let byte_pos = self.byte_offset();
        self.value.truncate(byte_pos);
    }

    /// Handle common text editing key events. Returns true if the key was handled.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('u') => self.delete_to_start(),
                KeyCode::Char('k') => self.delete_to_end(),
                _ => return false,
            }
            return true;
        }
        match key.code {
            KeyCode::Char(c) => self.insert_char(c),
            KeyCode::Backspace => self.delete_char(),
            KeyCode::Delete => self.delete_forward_char(),
            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::Home => self.move_to_start(),
            KeyCode::End => self.move_to_end(),
            _ => return false,
        }
        true
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let border_color = if self.focused {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        let display_value = if self.focused {
            let byte_pos = self.byte_offset();
            let before = &self.value[..byte_pos];
            let (cursor_char, after) = if byte_pos < self.value.len() {
                let ch = self.value[byte_pos..].chars().next().unwrap();
                let end = byte_pos + ch.len_utf8();
                (&self.value[byte_pos..end], &self.value[end..])
            } else {
                (" ", "")
            };
            Line::from(vec![
                Span::raw(before.to_string()),
                Span::styled(
                    cursor_char.to_string(),
                    Style::default().bg(Color::White).fg(Color::Black),
                ),
                Span::raw(after.to_string()),
            ])
        } else {
            Line::from(self.value.as_str())
        };

        // Compute display width of text before cursor + cursor char width for horizontal scroll
        let visible_width = area.width.saturating_sub(2) as usize;
        let cursor_display_col = self.value[..self.byte_offset()].width();
        let cursor_char_width = self.value[self.byte_offset()..]
            .chars()
            .next()
            .map(|ch| ch.width().unwrap_or(1))
            .unwrap_or(1);
        let cursor_end = cursor_display_col + cursor_char_width;
        let h_scroll = if visible_width > 0 && cursor_end > visible_width {
            (cursor_end - visible_width) as u16
        } else {
            0
        };

        let input = Paragraph::new(display_value)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", self.label))
                    .border_style(Style::default().fg(border_color)),
            )
            .scroll((0, h_scroll));
        frame.render_widget(input, area);
    }
}

/// Multi-line text input field (UTF-8 safe, supports Enter for newlines)
#[derive(Debug, Clone)]
pub struct TextArea {
    pub value: String,
    /// Cursor position as character index (not byte index) within the entire value
    pub cursor: usize,
    pub label: String,
    pub focused: bool,
}

impl TextArea {
    pub fn new(label: &str) -> Self {
        Self {
            value: String::new(),
            cursor: 0,
            label: label.to_string(),
            focused: false,
        }
    }

    pub fn with_value(mut self, value: &str) -> Self {
        self.value = value.to_string();
        self.cursor = value.chars().count();
        self
    }

    fn byte_offset(&self) -> usize {
        self.value
            .char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.value.len())
    }

    pub fn insert_char(&mut self, c: char) {
        let byte_pos = self.byte_offset();
        self.value.insert(byte_pos, c);
        self.cursor += 1;
    }

    pub fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    /// Insert a string (e.g. from paste), preserving newlines
    pub fn insert_paste(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        for c in normalized.chars() {
            self.insert_char(c);
        }
    }

    pub fn delete_char(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let byte_pos = self.byte_offset();
            self.value.remove(byte_pos);
        }
    }

    pub fn delete_forward_char(&mut self) {
        if self.cursor < self.value.chars().count() {
            let byte_pos = self.byte_offset();
            self.value.remove(byte_pos);
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.value.chars().count() {
            self.cursor += 1;
        }
    }

    /// Get (line_index, display_column_width, char_column_index) of the current cursor position.
    /// The display column is measured in display width (CJK characters count as 2).
    /// The char column is the character offset within the current line.
    fn cursor_line_col(&self) -> (usize, usize, usize) {
        let mut line = 0;
        let mut display_col = 0;
        let mut char_col = 0;
        for (i, ch) in self.value.chars().enumerate() {
            if i == self.cursor {
                return (line, display_col, char_col);
            }
            if ch == '\n' {
                line += 1;
                display_col = 0;
                char_col = 0;
            } else {
                display_col += ch.width().unwrap_or(0);
                char_col += 1;
            }
        }
        (line, display_col, char_col)
    }

    /// Get the lines of the value as (start_char_index, char_count, display_width) tuples
    fn line_ranges(&self) -> Vec<(usize, usize, usize)> {
        let mut ranges = Vec::new();
        let mut start = 0;
        let mut count = 0;
        let mut width = 0;
        for ch in self.value.chars() {
            if ch == '\n' {
                ranges.push((start, count, width));
                start = start + count + 1;
                count = 0;
                width = 0;
            } else {
                count += 1;
                width += ch.width().unwrap_or(0);
            }
        }
        ranges.push((start, count, width));
        ranges
    }

    /// Find the char index within a line that corresponds to a target display column.
    /// `line_start` is the char index of the first character of the line.
    fn char_index_at_display_col(&self, line_start: usize, line_char_count: usize, target_display_col: usize) -> usize {
        let mut display_col = 0;
        for (i, ch) in self.value.chars().skip(line_start).take(line_char_count).enumerate() {
            let w = ch.width().unwrap_or(0);
            if display_col + w > target_display_col {
                return line_start + i;
            }
            display_col += w;
        }
        line_start + line_char_count
    }

    pub fn move_up(&mut self) {
        let (line, col, _) = self.cursor_line_col();
        if line == 0 {
            return;
        }
        let ranges = self.line_ranges();
        let prev = &ranges[line - 1];
        let target_display_col = col.min(prev.2);
        self.cursor = self.char_index_at_display_col(prev.0, prev.1, target_display_col);
    }

    pub fn move_down(&mut self) {
        let (line, col, _) = self.cursor_line_col();
        let ranges = self.line_ranges();
        if line >= ranges.len() - 1 {
            return;
        }
        let next = &ranges[line + 1];
        let target_display_col = col.min(next.2);
        self.cursor = self.char_index_at_display_col(next.0, next.1, target_display_col);
    }

    pub fn move_to_line_start(&mut self) {
        let (line, _, _) = self.cursor_line_col();
        let ranges = self.line_ranges();
        self.cursor = ranges[line].0;
    }

    pub fn move_to_line_end(&mut self) {
        let (line, _, _) = self.cursor_line_col();
        let ranges = self.line_ranges();
        self.cursor = ranges[line].0 + ranges[line].1;
    }

    pub fn delete_to_line_start(&mut self) {
        let (line, col, _) = self.cursor_line_col();
        if col > 0 {
            let ranges = self.line_ranges();
            let line_start_char = ranges[line].0;
            let line_start_byte: usize = self.value.char_indices()
                .nth(line_start_char)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let cursor_byte = self.byte_offset();
            self.value.drain(line_start_byte..cursor_byte);
            self.cursor = line_start_char;
        }
    }

    pub fn delete_to_line_end(&mut self) {
        let cursor_byte = self.byte_offset();
        // Find the next newline or end of string
        let end_byte = self.value[cursor_byte..]
            .find('\n')
            .map(|pos| cursor_byte + pos)
            .unwrap_or(self.value.len());
        if cursor_byte == end_byte {
            // Already at end of line content — delete the newline to join lines
            if cursor_byte < self.value.len() && self.value.as_bytes()[cursor_byte] == b'\n' {
                // Newline is at cursor (middle of text) — remove it to join with next line
                self.value.remove(cursor_byte);
                // Move cursor to start of current line for consistent Ctrl+K chaining
                self.move_to_line_start();
            } else if cursor_byte > 0 && self.value.as_bytes()[cursor_byte - 1] == b'\n' {
                // Cursor is at start of last line (empty) — remove preceding newline
                self.value.remove(cursor_byte - 1);
                self.cursor -= 1;
                // Move cursor to the start of the (now current) line
                self.move_to_line_start();
            }
        } else {
            self.value.drain(cursor_byte..end_byte);
        }
    }

    /// Handle key events. Returns true if the key was handled.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Enter inserts newline (form submission is Ctrl+Enter, handled by modal)
        if key.code == KeyCode::Enter && !key.modifiers.contains(KeyModifiers::CONTROL) {
            self.insert_newline();
            return true;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('u') => self.delete_to_line_start(),
                KeyCode::Char('k') => self.delete_to_line_end(),
                _ => return false,
            }
            return true;
        }
        match key.code {
            KeyCode::Char(c) => self.insert_char(c),
            KeyCode::Backspace => self.delete_char(),
            KeyCode::Delete => self.delete_forward_char(),
            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::Up => self.move_up(),
            KeyCode::Down => self.move_down(),
            KeyCode::Home => self.move_to_line_start(),
            KeyCode::End => self.move_to_line_end(),
            _ => return false,
        }
        true
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let border_color = if self.focused {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        let text_lines: Vec<&str> = self.value.split('\n').collect();

        let display_lines: Vec<Line> = if self.focused {
            let (cursor_line, _cursor_display_col, cursor_char_col) = self.cursor_line_col();
            text_lines
                .iter()
                .enumerate()
                .map(|(line_idx, line_text)| {
                    if line_idx == cursor_line {
                        let chars: Vec<char> = line_text.chars().collect();
                        let before: String = chars[..cursor_char_col].iter().collect();
                        if cursor_char_col < chars.len() {
                            let cursor_char = chars[cursor_char_col].to_string();
                            let after: String = chars[cursor_char_col + 1..].iter().collect();
                            Line::from(vec![
                                Span::raw(before),
                                Span::styled(
                                    cursor_char,
                                    Style::default().bg(Color::White).fg(Color::Black),
                                ),
                                Span::raw(after),
                            ])
                        } else {
                            Line::from(vec![
                                Span::raw(before),
                                Span::styled(
                                    " ",
                                    Style::default().bg(Color::White).fg(Color::Black),
                                ),
                            ])
                        }
                    } else {
                        Line::from(line_text.to_string())
                    }
                })
                .collect()
        } else {
            text_lines.iter().map(|l| Line::from(l.to_string())).collect()
        };

        // Scroll to keep cursor visible
        let visible_height = area.height.saturating_sub(2) as usize;
        let visible_width = area.width.saturating_sub(2) as usize;
        let (cursor_line, cursor_col, _) = self.cursor_line_col();
        // Include cursor character width (CJK = 2, ASCII = 1, end-of-line cursor block = 1)
        let cursor_char_width = self.value.chars().nth(self.cursor)
            .and_then(|ch| if ch == '\n' { None } else { Some(ch) })
            .map(|ch| ch.width().unwrap_or(1))
            .unwrap_or(1);
        let cursor_end = cursor_col + cursor_char_width;
        let v_scroll = if visible_height > 0 && cursor_line >= visible_height {
            (cursor_line - visible_height + 1) as u16
        } else {
            0
        };
        let h_scroll = if visible_width > 0 && cursor_end > visible_width {
            (cursor_end - visible_width) as u16
        } else {
            0
        };

        let input = Paragraph::new(display_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", self.label))
                    .border_style(Style::default().fg(border_color)),
            )
            .scroll((v_scroll, h_scroll));
        frame.render_widget(input, area);
    }
}

/// Selection list component
#[derive(Debug, Clone)]
pub struct SelectList<T: Clone> {
    pub items: Vec<(String, T)>,
    pub selected: usize,
    pub label: String,
    pub focused: bool,
}

impl<T: Clone> SelectList<T> {
    pub fn new(label: &str, items: Vec<(String, T)>) -> Self {
        Self {
            items,
            selected: 0,
            label: label.to_string(),
            focused: false,
        }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.selected < self.items.len().saturating_sub(1) {
            self.selected += 1;
        }
    }

    pub fn selected_value(&self) -> Option<&T> {
        self.items.get(self.selected).map(|(_, v)| v)
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let border_color = if self.focused {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        let lines: Vec<Line> = self
            .items
            .iter()
            .enumerate()
            .map(|(i, (label, _))| {
                if i == self.selected {
                    Line::from(vec![
                        Span::styled(" > ", Style::default().fg(Color::Cyan)),
                        Span::styled(label.as_str(), Style::default().fg(Color::White)),
                    ])
                } else {
                    Line::from(vec![
                        Span::raw("   "),
                        Span::raw(label.as_str()),
                    ])
                }
            })
            .collect();

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", self.label))
                .border_style(Style::default().fg(border_color)),
        );
        frame.render_widget(paragraph, area);
    }
}

/// Multi-select list with checkboxes
#[derive(Debug, Clone)]
pub struct MultiSelectList<T: Clone> {
    pub items: Vec<(String, T, bool)>,
    pub cursor: usize,
    pub label: String,
    pub focused: bool,
    pub filter_text: String,
}

impl<T: Clone> MultiSelectList<T> {
    pub fn new(label: &str, items: Vec<(String, T)>) -> Self {
        let items = items.into_iter().map(|(l, v)| (l, v, false)).collect();
        Self {
            items,
            cursor: 0,
            label: label.to_string(),
            focused: false,
            filter_text: String::new(),
        }
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        if self.filter_text.is_empty() {
            return (0..self.items.len()).collect();
        }
        let query = self.filter_text.to_lowercase();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, (label, _, _))| label.to_lowercase().contains(&query))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        let max = self.filtered_indices().len().saturating_sub(1);
        if self.cursor < max {
            self.cursor += 1;
        }
    }

    pub fn toggle(&mut self) {
        let indices = self.filtered_indices();
        if let Some(&idx) = indices.get(self.cursor) {
            self.items[idx].2 = !self.items[idx].2;
        }
    }

    pub fn selected_values(&self) -> Vec<&T> {
        self.items
            .iter()
            .filter(|(_, _, selected)| *selected)
            .map(|(_, v, _)| v)
            .collect()
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let border_color = if self.focused {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        let indices = self.filtered_indices();
        let mut lines: Vec<Line> = Vec::new();

        // Filter input line
        if self.focused {
            lines.push(Line::from(vec![
                Span::styled("Filter: ", Style::default().fg(Color::DarkGray)),
                Span::styled(self.filter_text.as_str(), Style::default().fg(Color::White)),
            ]));
        }

        for (display_idx, &real_idx) in indices.iter().enumerate() {
            let (label, _, checked) = &self.items[real_idx];
            let checkbox = if *checked { "[x]" } else { "[ ]" };
            let is_cursor = display_idx == self.cursor;

            if is_cursor {
                lines.push(Line::from(vec![
                    Span::styled(" > ", Style::default().fg(Color::Cyan)),
                    Span::styled(
                        format!("{} {}", checkbox, label),
                        Style::default().fg(Color::White),
                    ),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::raw("   "),
                    Span::raw(format!("{} {}", checkbox, label)),
                ]));
            }
        }

        // Calculate scroll offset to keep cursor visible
        // Available height = area height - 2 (borders)
        let visible_height = area.height.saturating_sub(2) as usize;
        // The cursor line index within `lines` is offset by 1 if filter line is shown
        let cursor_line = if self.focused {
            self.cursor + 1 // +1 for the filter line
        } else {
            self.cursor
        };
        let scroll_offset = if visible_height > 0 && cursor_line >= visible_height {
            (cursor_line - visible_height + 1) as u16
        } else {
            0
        };

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", self.label))
                    .border_style(Style::default().fg(border_color)),
            )
            .scroll((scroll_offset, 0));
        frame.render_widget(paragraph, area);
    }
}
