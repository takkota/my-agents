use crate::app::FocusPane;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// What kind of item is currently selected in the task tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionContext {
    Project,
    Task,
    None,
}

pub struct StatusBar;

impl StatusBar {
    pub fn render_main(
        frame: &mut Frame,
        area: Rect,
        error_msg: Option<&str>,
        context: SelectionContext,
        focus: FocusPane,
    ) {
        if let Some(err) = error_msg {
            let line = Line::from(vec![
                Span::styled(
                    " ERROR: ",
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(err, Style::default().fg(Color::Red)),
            ]);
            let bar = Paragraph::new(line).style(Style::default().bg(Color::Rgb(30, 30, 46)));
            frame.render_widget(bar, area);
            return;
        }

        let key_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        let desc_style = Style::default().fg(Color::Gray);

        let mut spans = Vec::new();

        // Focus hint
        spans.extend([
            Span::styled(" w", key_style),
            Span::styled(" Focus ", desc_style),
        ]);

        // When right panes are focused, show scroll hint
        if matches!(focus, FocusPane::InfoPanel | FocusPane::SessionPanel) {
            spans.extend([
                Span::styled("j/k", key_style),
                Span::styled(" Scroll ", desc_style),
            ]);
        }
        if focus == FocusPane::SessionPanel {
            spans.extend([
                Span::styled("G", key_style),
                Span::styled(" Bottom ", desc_style),
            ]);
        }

        // Common keys (always available)
        spans.extend([
            Span::styled("p", key_style),
            Span::styled(" Project ", desc_style),
            Span::styled("n", key_style),
            Span::styled(" Task ", desc_style),
        ]);

        // m/d require a selection
        if matches!(context, SelectionContext::Project | SelectionContext::Task) {
            spans.extend([
                Span::styled("m", key_style),
                Span::styled(" Edit ", desc_style),
                Span::styled("d", key_style),
                Span::styled(" Delete ", desc_style),
            ]);
        }

        match context {
            SelectionContext::Project => {
                spans.extend([
                    Span::styled("o", key_style),
                    Span::styled(" Toggle ", desc_style),
                    Span::styled("f", key_style),
                    Span::styled(" Filter ", desc_style),
                    Span::styled("A", key_style),
                    Span::styled(" Action ", desc_style),
                    Span::styled("s", key_style),
                    Span::styled(" Sort ", desc_style),
                    Span::styled("M", key_style),
                    Span::styled(" PM ", desc_style),
                    Span::styled("C", key_style),
                    Span::styled(" Settings ", desc_style),
                    Span::styled("Enter", key_style),
                    Span::styled(" Attach(PM) ", desc_style),
                    Span::styled("q", key_style),
                    Span::styled(" Quit", desc_style),
                ]);
            }
            SelectionContext::Task => {
                spans.extend([
                    Span::styled("S", key_style),
                    Span::styled(" Status ", desc_style),
                    Span::styled("L", key_style),
                    Span::styled(" Link ", desc_style),
                    Span::styled("o", key_style),
                    Span::styled(" Open ", desc_style),
                    Span::styled("v", key_style),
                    Span::styled(" Preview ", desc_style),
                    Span::styled("f", key_style),
                    Span::styled(" Filter ", desc_style),
                    Span::styled("A", key_style),
                    Span::styled(" Action ", desc_style),
                    Span::styled("s", key_style),
                    Span::styled(" Sort ", desc_style),
                    Span::styled("R", key_style),
                    Span::styled(" Review ", desc_style),
                    Span::styled("P", key_style),
                    Span::styled(" PR ", desc_style),
                    Span::styled("U", key_style),
                    Span::styled(" Prompt ", desc_style),
                    Span::styled("1-5", key_style),
                    Span::styled(" Priority ", desc_style),
                    Span::styled("M", key_style),
                    Span::styled(" PM ", desc_style),
                    Span::styled("C", key_style),
                    Span::styled(" Settings ", desc_style),
                    Span::styled("Enter", key_style),
                    Span::styled(" Attach ", desc_style),
                    Span::styled("q", key_style),
                    Span::styled(" Quit", desc_style),
                ]);
            }
            SelectionContext::None => {
                spans.extend([
                    Span::styled("f", key_style),
                    Span::styled(" Filter ", desc_style),
                    Span::styled("A", key_style),
                    Span::styled(" Action ", desc_style),
                    Span::styled("s", key_style),
                    Span::styled(" Sort ", desc_style),
                    Span::styled("C", key_style),
                    Span::styled(" Settings ", desc_style),
                    Span::styled("q", key_style),
                    Span::styled(" Quit", desc_style),
                ]);
            }
        }

        let hints = Line::from(spans);
        let bar = Paragraph::new(hints).style(Style::default().bg(Color::Rgb(30, 30, 46)));
        frame.render_widget(bar, area);
    }

    pub fn render_modal(frame: &mut Frame, area: Rect) {
        let hints = Line::from(vec![
            Span::styled(
                " Esc/Ctrl+C",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Cancel ", Style::default().fg(Color::Gray)),
            Span::styled(
                "Ctrl+Enter",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Confirm ", Style::default().fg(Color::Gray)),
            Span::styled(
                "Tab",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Next Field", Style::default().fg(Color::Gray)),
        ]);
        let bar = Paragraph::new(hints).style(Style::default().bg(Color::Rgb(30, 30, 46)));
        frame.render_widget(bar, area);
    }
}
