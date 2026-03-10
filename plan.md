# タスク管理エージェントスキル実装計画

## 概要
AI agentがセッション内からmy-agentsのタスク管理操作を行えるようにする。
CLIスクリプト + スキル定義ファイルで実現。

## 実装内容

### 1. CLIスクリプト `scripts/ma-task` (シェルスクリプト)

タスクIDの自動検出とタスク操作を提供するCLIツール。
JSONの読み書きは `jq` を使用（なければ `python3` にフォールバック）。

#### サブコマンド:

| コマンド | 説明 |
|---------|------|
| `ma-task current` | CWDから現在のproject_id, task_id, task詳細を自動検出して表示 |
| `ma-task get <task-id>` | 指定タスクの詳細をJSON出力 |
| `ma-task list [--project <id>]` | タスク一覧表示（プロジェクト指定可） |
| `ma-task create --project <id> --name <name> [--priority P3] [--agent claude\|codex\|none]` | 新タスク作成 |
| `ma-task update <task-id> [--name <name>] [--priority <p>] [--notes <text>]` | タスク更新 |
| `ma-task status <task-id> <status>` | ステータス変更 (Todo/InProgress/InReview/Completed/Blocked) |
| `ma-task link <task-id> <url> [--name <display-name>]` | リンク追加 |

#### タスクID自動検出ロジック:
```
CWD → ~/.my-agents/projects/{project_id}/tasks/{task_id}[/...]
正規表現: .my-agents/projects/([^/]+)/tasks/([^/]+)
```

`ma-task current` は引数なしで呼べるため、エージェントは最初にこれを実行して自分のコンテキストを把握できる。

#### 設計ポイント:
- `--json` フラグでJSON出力（デフォルト）、`--human` で人間向けフォーマット
- task.jsonの直接読み書き（FsStoreと同じディレクトリ構造を使用）
- updated_atは変更時に自動更新
- 新規作成時はUUID v4の先頭8文字をIDに使用（既存ロジックと同じ）

### 2. タスクディレクトリへのCLAUDE.md追記

`fs_store.rs`の`write_agent_config_files`を修正し、タスクディレクトリのCLAUDE.mdにスキル説明を追記。
エージェントが起動時にma-taskコマンドの存在と使い方を認識できるようにする。

内容:
- `ma-task` コマンドの使い方
- `ma-task current` で自分のタスク情報を取得できること
- ステータス変更やリンク追加の方法
- 新しいタスク（サブタスク）の作成方法

### 3. AGENTS.md（Codex向け）の同様の追記

Codex用にも同じスキル情報をAGENTS.mdに追記。

### 4. スクリプトのインストール

- `scripts/ma-task` をリポジトリに配置
- `cargo install --path .` 実行時にスクリプトも `~/.my-agents/bin/` にコピー
  → もしくはスクリプト内でdata_dirを自動検出してPATH不要にする
- 代替案: `ma-task` をスタンドアロンにし、`~/.my-agents/` の場所はconfig.tomlから読むか、デフォルトの `~/.my-agents/` を使用

## ファイル変更一覧

| ファイル | 変更内容 |
|---------|---------|
| `scripts/ma-task` | **新規** - CLIスクリプト本体 |
| `src/storage/fs_store.rs` | `write_agent_config_files` にスキル説明の追記ロジック追加 |
| `src/app.rs` | タスク作成時に `ma-task` スクリプトをタスクディレクトリへコピー（もしくはPATH設定） |
| `CLAUDE.md` | ma-taskコマンドのドキュメント追加 |

## タスクID検出 → task.json アクセスの流れ

```
Agent起動（CWD = ~/.my-agents/projects/proj1/tasks/abc12345/my-repo/）
  ↓
ma-task current
  ↓
CWDから project_id=proj1, task_id=abc12345 を抽出
  ↓
~/.my-agents/projects/proj1/tasks/abc12345/task.json を読み取り
  ↓
タスク詳細をJSON出力
```

## スクリプトをエージェントに認識させる方法

タスクディレクトリの CLAUDE.md / AGENTS.md に以下を記述:

```markdown
## Task Management

このセッションはmy-agentsタスク管理システムの一部です。
`ma-task` コマンドでタスク操作が可能です。

- `ma-task current` - 現在のタスク情報を取得
- `ma-task status <task-id> InReview` - ステータス変更
- `ma-task link <task-id> <url>` - PRやissueのリンクを追加
- `ma-task create --project <project-id> --name "サブタスク名"` - サブタスク作成
- `ma-task list --project <project-id>` - プロジェクト内タスク一覧
```
