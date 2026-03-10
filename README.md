# my-agents

AIコーディングエージェント向けのTUIベースTODOリスト管理ツール。

複数のAIエージェントセッション（Claude Code / Codex）を一元管理し、プロジェクト単位でタスクを整理できます。

## Features

- **プロジェクト/タスク管理** - ツリー表示でプロジェクト配下のタスクを一覧・作成・編集・削除
- **tmuxセッション統合** - タスクごとにtmuxセッションを自動作成、ワンキーでattach/detach
- **セッションプレビュー** - メイン画面右側で選択タスクのtmuxセッション内容をリアルタイム表示
- **git worktree管理** - タスクごとに複数リポジトリのworktreeを自動作成・クリーンアップ
- **Agent CLI起動** - タスク作成時にClaude Code / Codexを自動起動
- **ステータス管理** - Todo / In Progress / In Review / Completed / Blocked
- **リンク管理** - GitHub Issue/PRのURLを紐付け、自動で見やすい表示名を生成
- **フィルタ/ソート** - ステータスフィルタ、作成日・更新日・Priority順ソート
- **Agent状態監視** - エージェントの入力待ち状態を検知してステータスを自動更新
- **CLAUDE.md/AGENTS.md参照** - worktree作成時に設定ファイルへの参照を自動生成

## Requirements

- Rust 1.70+
- tmux
- git
- [fd](https://github.com/sharkdp/fd) (推奨) - gitリポジトリ検索を高速化。未インストールでも動作しますが、`find`より約5倍高速です
  ```bash
  brew install fd  # macOS
  ```

## Install

```bash
cargo install --path .
```

## Usage

```bash
my-agents
```

初回起動時にサンプルプロジェクト（quickstart）が自動作成されます。

## Key Bindings

### メイン画面

| Key | Action |
|-----|--------|
| `j` / `k` / `↑` / `↓` | カーソル移動 |
| `Enter` | tmuxセッションにattach / プロジェクト開閉 |
| `p` | プロジェクト作成 |
| `n` | タスク作成 |
| `m` | 編集 |
| `d` | 削除 |
| `S` | ステータス変更 |
| `L` | リンク追加 |
| `f` | フィルタ |
| `s` | ソート |
| `q` | 終了 |

### モーダル

| Key | Action |
|-----|--------|
| `Tab` | 次のフィールド |
| `Enter` | 確定 |
| `Esc` | キャンセル |
| `Space` | チェックボックス切替（リポジトリ選択等） |

### tmuxセッション内

| Key | Action |
|-----|--------|
| `Ctrl+Q` | detachしてメイン画面に戻る |

## Directory Structure

```
~/.my-agents/
├── config.toml                          # 設定ファイル
└── projects/
    └── {project_name}/
        ├── project.json                 # プロジェクトメタデータ
        └── tasks/
            └── {task_id}/
                ├── task.json            # タスクメタデータ
                ├── CLAUDE.md            # @repo/CLAUDE.md 参照
                ├── AGENTS.md            # @repo/AGENTS.md 参照
                └── {repo_name}/         # git worktree
```

## Configuration

`~/.my-agents/config.toml`:

```toml
# デフォルトのAgent CLI (Claude / Codex / None)
default_agent_cli = "Claude"

# Tick間隔 (ms)
tick_rate_ms = 250

# Agent状態監視間隔 (秒)
monitor_interval_secs = 10
```

## Tech Stack

- [Rust](https://www.rust-lang.org/)
- [ratatui](https://ratatui.rs/) - TUIフレームワーク
- [crossterm](https://github.com/crossterm-rs/crossterm) - ターミナルバックエンド
- [tokio](https://tokio.rs/) - 非同期ランタイム
- tmux - セッション管理
- git worktree - ブランチ分離

## License

MIT
