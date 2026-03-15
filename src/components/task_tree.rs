use crate::domain::project::Project;
use crate::domain::task::{Status, Task};
use crate::services::tmux::TmuxService;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub enum TreeItem {
    Project {
        id: String,
        name: String,
        task_count: usize,
        pm_active: bool,
    },
    Task {
        id: String,
        project_id: String,
        name: String,
        status: Status,
        priority: crate::domain::task::Priority,
        has_session: bool,
        links: Vec<crate::domain::task::TaskLink>,
        notes: Option<String>,
    },
}

impl TreeItem {
    pub fn project_id(&self) -> &str {
        match self {
            TreeItem::Project { id, .. } => id,
            TreeItem::Task { project_id, .. } => project_id,
        }
    }

}

pub struct TaskTree {
    pub items: Vec<TreeItem>,
    pub state: ListState,
    pub expanded: HashSet<String>,
    pub status_filter: Option<Vec<Status>>,
    pub sort_mode: SortMode,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SortMode {
    CreatedDesc,
    UpdatedDesc,
    PriorityDesc,
}

impl Default for SortMode {
    fn default() -> Self {
        SortMode::PriorityDesc
    }
}

impl TaskTree {
    pub fn new(sort_mode: SortMode) -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            items: Vec::new(),
            state,
            expanded: HashSet::new(),
            status_filter: None,
            sort_mode,
        }
    }

    pub fn rebuild(
        &mut self,
        projects: &[Project],
        tasks_by_project: &HashMap<String, Vec<Task>>,
        active_sessions: &HashSet<String>,
    ) {
        let selected_item = self.selected_item().cloned();
        self.items.clear();

        for project in projects {
            let project_tasks = tasks_by_project
                .get(&project.id)
                .cloned()
                .unwrap_or_default();

            let filtered_tasks: Vec<&Task> = project_tasks
                .iter()
                .filter(|t| {
                    if let Some(filter) = &self.status_filter {
                        filter.contains(&t.status)
                    } else {
                        true
                    }
                })
                .collect();

            let pm_session = TmuxService::pm_session_name(&project.id);
            let pm_active = active_sessions.contains(&pm_session);

            self.items.push(TreeItem::Project {
                id: project.id.clone(),
                name: project.name.clone(),
                task_count: filtered_tasks.len(),
                pm_active,
            });

            if self.expanded.contains(&project.id) {
                let mut sorted_tasks = filtered_tasks;
                match self.sort_mode {
                    SortMode::CreatedDesc => {
                        sorted_tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at))
                    }
                    SortMode::UpdatedDesc => {
                        sorted_tasks.sort_by(|a, b| b.updated_at.cmp(&a.updated_at))
                    }
                    SortMode::PriorityDesc => {
                        sorted_tasks.sort_by(|a, b| a.priority.cmp(&b.priority))
                    }
                }

                for task in sorted_tasks {
                    let has_session = task
                        .tmux_session
                        .as_ref()
                        .map(|s| active_sessions.contains(s))
                        .unwrap_or(false);

                    self.items.push(TreeItem::Task {
                        id: task.id.clone(),
                        project_id: task.project_id.clone(),
                        name: task.name.clone(),
                        status: task.status,
                        priority: task.priority,
                        has_session,
                        links: task.links.clone(),
                        notes: task.notes.clone(),
                    });
                }
            }
        }

        // Restore selection
        if let Some(prev) = &selected_item {
            let idx = self.items.iter().position(|item| match (item, prev) {
                (TreeItem::Project { id: a, .. }, TreeItem::Project { id: b, .. }) => a == b,
                (TreeItem::Task { id: a, .. }, TreeItem::Task { id: b, .. }) => a == b,
                _ => false,
            });
            self.state.select(Some(idx.unwrap_or(0)));
        }

        if self.items.is_empty() {
            self.state.select(None);
        } else if self.state.selected().is_none() {
            self.state.select(Some(0));
        }
    }

    pub fn select_task_by_id(&mut self, task_id: &str) {
        if let Some(idx) = self.items.iter().position(|item| matches!(item, TreeItem::Task { id, .. } if id == task_id)) {
            self.state.select(Some(idx));
        }
    }

    pub fn selected_item(&self) -> Option<&TreeItem> {
        self.state.selected().and_then(|i| self.items.get(i))
    }

    pub fn move_up(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn move_down(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn toggle_expand(&mut self) {
        if let Some(item) = self.selected_item().cloned() {
            if let TreeItem::Project { id, .. } = &item {
                if self.expanded.contains(id) {
                    self.expanded.remove(id);
                } else {
                    self.expanded.insert(id.clone());
                }
            }
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let items: Vec<ListItem> = self
            .items
            .iter()
            .map(|item| match item {
                TreeItem::Project {
                    name, task_count, id, pm_active, ..
                } => {
                    let arrow = if self.expanded.contains(id) {
                        "▼"
                    } else {
                        "▶"
                    };
                    let mut spans = vec![
                        Span::styled(
                            format!(" {} ", arrow),
                            Style::default().fg(Color::Yellow),
                        ),
                        Span::styled(
                            format!("{} ", name),
                            Style::default()
                                .fg(Color::White)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("({})", task_count),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ];
                    if *pm_active {
                        spans.push(Span::styled(
                            " [PM]",
                            Style::default().fg(Color::Green),
                        ));
                    }
                    let line = Line::from(spans);
                    ListItem::new(line)
                }
                TreeItem::Task {
                    name,
                    status,
                    priority,
                    has_session,
                    links,
                    ..
                } => {
                    let status_color = match status {
                        Status::Todo => Color::Gray,
                        Status::InProgress => Color::Blue,
                        Status::ActionRequired => Color::LightRed,
                        Status::Completed => Color::Green,
                        Status::Blocked => Color::Red,
                    };
                    let session_indicator = if *has_session { "⚡" } else { "  " };
                    let link_text = if !links.is_empty() {
                        let displays: Vec<String> = links.iter().map(|l| l.display()).collect();
                        format!(" 🔗{}", displays.join(","))
                    } else {
                        String::new()
                    };

                    let line = Line::from(vec![
                        Span::raw("   "),
                        Span::styled(
                            format!("{} ", status.symbol()),
                            Style::default().fg(status_color),
                        ),
                        Span::styled(
                            format!("{} ", name),
                            Style::default().fg(Color::White),
                        ),
                        Span::styled(
                            format!("[{}] ", priority),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::styled(
                            session_indicator.to_string(),
                            Style::default().fg(Color::Yellow),
                        ),
                        Span::styled(link_text, Style::default().fg(Color::Blue)),
                    ]);
                    ListItem::new(line)
                }
            })
            .collect();

        let border_color = if focused { Color::Yellow } else { Color::Reset };
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Projects & Tasks ")
                    .border_style(Style::default().fg(border_color)),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            );

        frame.render_stateful_widget(list, area, &mut self.state);
    }
}
