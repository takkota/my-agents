# my-agents

> **⚠️ 注意**: このツールは個人のユースケースやワークフローに合わせて作成したものであり、カスタマイズ性はほとんどありません。また、厳密なバージョン管理（セマンティックバージョニング等）は行っておらず、破壊的変更が予告なく入る可能性があります。第三者の利用は推奨しません。

AIコーディングエージェント向けのTUIベースTODOリスト管理ツール。

複数のAIエージェントセッション（Claude Code / Codex / Gemini CLI）を一元管理し、プロジェクト単位でタスクを整理できます。

## Features

- **プロジェクト/タスク管理** - ツリー表示でプロジェクト配下のタスクを一覧・作成・編集・削除
- **tmuxセッション統合** - タスクごとにtmuxセッションを自動作成、ワンキーでattach/detach
- **セッションプレビュー** - メイン画面右側で選択タスクのtmuxセッション内容をリアルタイム表示
- **git worktree管理** - タスクごとに複数リポジトリのworktreeを自動作成・クリーンアップ
- **Agent CLI起動** - タスク作成時にClaude Code / Codex / Gemini CLIを自動起動
- **ステータス管理** - Todo / In Progress / Action Required / Completed / Blocked
- **リンク管理** - GitHub Issue/PRのURLを紐付け、自動で見やすい表示名を生成
- **フィルタ/ソート** - ステータスフィルタ、作成日・更新日・Priority順ソート
- **Agent状態監視** - エージェントの入力待ち状態を検知してステータスを自動更新
- **PRマージ監視** - GitHub PRのマージを検知してタスクを自動完了
- **CLAUDE.md/AGENTS.md/GEMINI.md参照** - worktree作成時に設定ファイルへの参照を自動生成
- **Agent Skills** - Claude Code (`.claude/skills/`) / Codex (`.agents/skills/`) 両対応のスキルファイルを自動生成。Gemini CLIはGEMINI.mdで指示を提供。エージェントが `ma-task` CLIでタスク管理可能
- **ma-task CLI** - エージェント向けタスク管理CLI。ステータス更新・リンク追加・タスク作成・既存タスク実行・タスク削除等をJSON出力で提供

## Requirements

- Rust 1.70+
- [tmux](https://github.com/tmux/tmux) - セッション管理に必須
  ```bash
  brew install tmux    # macOS
  sudo apt install tmux  # Ubuntu/Debian
  sudo dnf install tmux  # Fedora
  sudo pacman -S tmux    # Arch Linux
  ```
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
| `S` / `$` | ステータス変更 |
| `L` | リンク追加 |
| `o` | リンクをブラウザで開く |
| `f` | フィルタ |
| `A` | Action Requiredでフィルタ |
| `s` | ソート |
| `1` - `5` | Priority設定（P1〜P5） |
| `P` | エージェントにPR作成を指示 |
| `R` | エージェントにレビューを指示 |
| `q` | 終了 |

### Emacs風キーバインド（グローバル）

| Key | Action |
|-----|--------|
| `Ctrl+N` / `Ctrl+P` | ↓ / ↑ |
| `Ctrl+F` / `Ctrl+B` | → / ← |
| `Ctrl+A` / `Ctrl+E` | Home / End |
| `Ctrl+H` / `Ctrl+D` | Backspace / Delete |

### モーダル

| Key | Action |
|-----|--------|
| `Ctrl+Enter` | 確定（フォーム送信） |
| `Tab` / `Shift+Tab` | 次 / 前のフィールド |
| `Enter` | 選択確定（ステータス・ソート等） / テキストエリア内改行 |
| `Esc` / `Ctrl+C` | キャンセル |
| `Space` | チェックボックス切替（リポジトリ選択・フィルタ等） |
| `j` / `k` | リスト内カーソル移動 |

### テキスト入力

| Key | Action |
|-----|--------|
| `Ctrl+U` | カーソルから行頭まで削除 |
| `Ctrl+K` | カーソルから行末まで削除 |

### tmuxセッション内

| Key | Action |
|-----|--------|
| `Ctrl+Q` | detachしてメイン画面に戻る |

## Directory Structure

```
~/.my-agents/
├── config.toml                          # 設定ファイル
├── bin/
│   ├── ma-task                          # エージェント向けタスク管理CLI
│   └── ma-codex-notify                  # Codex notify イベントハンドラ
└── projects/
    └── {project_name}/
        ├── project.json                 # プロジェクトメタデータ
        └── tasks/
            └── {task_id}/
                ├── task.json            # タスクメタデータ
                ├── CLAUDE.md            # @repo/CLAUDE.md 参照 + スキルトリガー
                ├── AGENTS.md            # @repo/AGENTS.md 参照 + スキルトリガー
                ├── .claude/
                │   ├── settings.json    # Claude Code hooks設定
                │   └── skills/
                │       └── task-management/
                │           └── SKILL.md # Claude Code用スキル
                ├── .agents/
                │   └── skills/
                │       └── task-management/
                │           └── SKILL.md # Codex用スキル
                ├── .codex/
                │   └── config.toml      # Codex notify設定
                └── {repo_name}/         # git worktree
```

## tmux設定（Shift+Enter対応）

tmux内でClaude Codeを使う場合、Shift+Enterによる改行がデフォルトでは動作しません。tmuxがモディファイアキーのエスケープシーケンスを透過しないためです。

`~/.tmux.conf` に以下を追加してください:

```bash
set -s extended-keys always
set -as terminal-features ',*:extkeys'
```

設定後、tmuxサーバーを再起動する必要があります:

```bash
tmux kill-server
```

> **Note**: tmux 3.2以降が必要です。`tmux -V` でバージョンを確認してください。

## Configuration

`~/.my-agents/config.toml`:

```toml
# デフォルトのAgent CLI (Claude / Codex / Gemini / None)
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
