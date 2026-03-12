# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Run Commands

```bash
cargo build                # Build
cargo run                  # Run the TUI app (requires tmux and git)
cargo test                 # Run all tests
cargo test <test_name>     # Run a single test
cargo clippy               # Lint
cargo fmt                  # Format
cargo install --path .     # Install binary as `my-agents`
```

## What This Is

A TUI-based task manager for AI coding agents (Claude Code / Codex). It manages multiple agent sessions per project, each with its own tmux session and git worktree. Data is stored as JSON files under `~/.my-agents/`.

## Architecture

### Event Loop (main.rs)
Standard ratatui event loop: `render → wait for event → handle_key_event → update → repeat`. Terminal is suspended/restored around tmux attach operations.

### App (app.rs) — Central Coordinator
Owns all state: projects, tasks, UI components, services. Implements a two-phase update pattern:
1. `handle_key_event()` → translates `KeyEvent` into an `Action` enum
2. `update(Action)` → performs side effects (CRUD, tmux, worktree) and returns `UpdateResult` (Continue or AttachSession)

Modals take input priority when active. Ctrl+N/P/F/B/A/E are remapped to arrow/cursor keys globally.

### Action (action.rs)
Single enum representing all possible state transitions. Modal `handle_key()` methods return `Action` variants to communicate back to `App`.

### Domain (domain/)
- `Task` — has id (8-char UUID prefix), status (Todo/InProgress/ActionRequired/Completed/Blocked), priority (P1-P5), agent_cli (Claude/Codex/None), worktrees, links
- `Project` — groups tasks, references git repos (`RepoRef`), configures `worktree_copy_files`

### Storage (storage/fs_store.rs)
`FsStore` reads/writes JSON files under `~/.my-agents/projects/{project}/tasks/{task_id}/task.json`. No database.

On startup, `install_scripts()` embeds the `ma-task` bash script (via `include_str!`) into `~/.my-agents/bin/`. The script is auto-updated when the binary version changes.

When creating agent sessions, `write_agent_config_files()` generates:
- **CLAUDE.md** / **AGENTS.md** — `@repo/` references to upstream config + skill trigger description
- **Claude Code skill** — `.claude/skills/task-management/SKILL.md` (with `allowed-tools: Bash`)
- **Codex skill** — `.agents/skills/task-management/SKILL.md` (standard Agent Skills format)
- **Claude hooks** — `.claude/settings.json` with `UserPromptSubmit`, `Stop`, and `PostToolUse` hooks for auto status tracking and PR link discovery
- **Codex notify** — writes `.codex/config.toml` in the task directory with `notify` pointing to `ma-codex-notify` (project-level config, no global config modification)
- Both skills share the same body via `skill_body()` helper, differing only in frontmatter

### Services (services/)
- `TmuxService` — create/kill/attach sessions, capture pane content, launch agent CLI
- `WorktreeService` — create/remove git worktrees per task, branch naming: `task/{short_id}/{repo_name}`
- `AgentMonitor` — periodically checks marker files to detect status transitions, auto-updates task status
- `PrMonitor` — background thread checks GitHub PR merge status via `gh` CLI, auto-completes tasks
- `git_finder` — discovers git repos using `fd` (fallback: `find`)

### Components (components/)
- `TaskTree` — left panel, tree view of projects/tasks with filtering and sorting
- `PreviewPanel` — right panel, shows selected task's tmux session content
- `StatusBar` — bottom bar with key hints or error messages
- `modals/` — each modal implements the `Modal` trait (`handle_key`, `render`). Uses `TextInput` widget for text fields. Returns `Action` on confirm, `CloseModal` on Esc.

### Modal Trait Pattern
All modals implement `Modal` trait from `components/modals/mod.rs`. They handle their own key events and return `Option<Action>`. `ModalKind` enum in `app.rs` wraps all modal types for dispatch.

## Key Conventions

- All text input is UTF-8 safe (cursor tracks char indices, not byte indices)
- Task IDs are first 8 chars of UUID v4
- tmux session names follow pattern: `ma-{project_id}-{task_id_prefix}`
- Worktree branches: `{task_id_6char}` (first 6 chars of task ID, no slashes)
- `.prompt_submitted` marker in task dir — created by `UserPromptSubmit` hook (Claude) or notify script (Codex) when user sends a prompt; triggers Todo/Completed/ActionRequired → InProgress
- `.agent_stopped` marker in task dir — created by `Stop` hook (Claude) or notify script (Codex) when agent finishes responding; triggers InProgress → ActionRequired
- `config.toml` at `~/.my-agents/config.toml` controls defaults (agent CLI, tick rate, monitor intervals)
- `ma-task` CLI (bash script in `~/.my-agents/bin/`) lets agents manage tasks via JSON commands
- `ma-codex-notify` script (bash in `~/.my-agents/bin/`) handles Codex `notify` events for automatic status tracking
- Agent skills are written per-agent format: `.claude/skills/` for Claude Code, `.agents/skills/` for Codex
