# ma-task CLI

AI エージェントおよび外部サービスからタスクを管理するための CLI ツール。
`~/.my-agents/projects/` 配下の JSON ファイルを読み書きしてタスクの CRUD を行う。

## 前提条件

- **jq** (必須) — JSON の生成・パースに使用
- **my-agents** バイナリ (`--run` フラグ使用時のみ) — worktree/tmux/エージェント起動に必要

## インストール場所

`my-agents` TUI バイナリの初回起動時に `~/.my-agents/bin/ma-task` へ自動インストールされる。
PATH に `~/.my-agents/bin` を追加するか、直接パスで呼び出す。

## 環境変数

| 変数 | 説明 | デフォルト |
|---|---|---|
| `MY_AGENTS_DATA_DIR` | データディレクトリ | `~/.my-agents` |
| `MY_AGENTS_BIN` | `my-agents` バイナリのパス (`--run` 時に優先使用) | PATH から検索 |

## データ構造

```
~/.my-agents/
  projects/
    {project_id}/
      project.json
      tasks/
        {task_id}/
          task.json
          .initial_prompt        # --prompt で生成 (エージェント起動時に使用)
          .prompt_submitted      # エージェントがプロンプトを受信したマーカー
          .agent_stopped         # エージェントが応答完了したマーカー
```

## コマンド一覧

### create — タスク作成

```bash
ma-task create --name <name> [options]
```

| オプション | 短縮 | 必須 | デフォルト | 説明 |
|---|---|---|---|---|
| `--project` | `-p` | \* | CWD から自動検出 | プロジェクト ID |
| `--name` | `-n` | Yes | — | タスク名 |
| `--priority` | — | No | `P3` | `P1` `P2` `P3` `P4` `P5` |
| `--agent` | — | No | `Claude` | `Claude` `Codex` `None` |
| `--notes` | — | No | `null` | メモ |
| `--prompt` | — | No | `null` | エージェントへの初期プロンプト |
| `--link` | — | No | — | リンク URL (複数回指定可) |
| `--run` | — | No | `false` | 作成後に worktree・tmux セッション・エージェント起動を即時実行 |

**出力:** 作成された task.json (JSON)

`--run` 指定時は task.json 出力後に `my-agents setup-task` を `exec` で呼び出す。
`setup-task` の出力 (worktree パス・tmux セッション名を含む JSON) が追加で stdout に出力される。

**例:**

```bash
# 基本的なタスク作成
ma-task create --project myproj --name "Fix login bug"

# リンク付きタスク作成
ma-task create --project myproj --name "Fix login bug" \
  --link "https://github.com/owner/repo/issues/42" \
  --link "https://github.com/owner/repo/pull/43"

# エージェント付きタスク作成 + 即時実行
ma-task create \
  --project myproj \
  --name "Fix login timeout" \
  --agent Claude \
  --priority P2 \
  --prompt "auth.rs のログインタイムアウト問題を修正してください" \
  --run
```

### get — タスク取得

```bash
ma-task get <task-id>
```

タスク ID は前方一致で検索される (6文字以上推奨)。CWD がタスクディレクトリ内の場合、そのプロジェクトを優先検索する。

**出力:** task.json (JSON)

### list — タスク一覧

```bash
ma-task list [--project <id>]
```

**出力:** task.json の JSON 配列

### current — 現在のタスク

```bash
ma-task current
```

CWD から自動検出。タスクディレクトリ内で実行する必要がある。

**出力:** task.json (JSON)

### update — タスク更新

```bash
ma-task update <task-id> [--name <name>] [--priority <P1-P5>] [--notes <text>] [--agent <Claude|Codex|None>]
```

指定フィールドのみ更新。`updated_at` は自動更新。

**出力:** 更新後の task.json (JSON)

### status — ステータス変更

```bash
ma-task status <task-id> <status>
```

**有効なステータス値:**

| 値 | エイリアス |
|---|---|
| `Todo` | `todo` |
| `InProgress` | `inprogress`, `in-progress`, `in_progress` |
| `ActionRequired` | `actionrequired`, `action-required`, `action_required` |
| `Completed` | `completed`, `done` |
| `Blocked` | `blocked` |

現在のステータスと同じ場合は書き込みをスキップする。

**出力:** 更新後の task.json (JSON)

### link — リンク追加

```bash
ma-task link <task-id> <url> [--name <display-name>]
```

**出力:** 更新後の task.json (JSON)

### run — 既存タスクの実行

```bash
ma-task run <task-id>
```

既存タスクに対して worktree 作成・tmux セッション起動・エージェント起動を行う。`create --run` と同等の処理を、既に作成済みのタスクに対して実行する。

- タスクに worktree や tmux セッションが**ない**場合は `my-agents setup-task` に委譲して新規セットアップを行う
- タスクに既に worktree や tmux セッションがある場合は `my-agents launch-agent` に委譲し、既存セッション内でエージェントを起動する（initial instructions + links 付き）。セッションが消失していた場合は自動再作成する

**出力 (新規セットアップ時):** `setup-task` の出力 (worktree パス・tmux セッション名を含む JSON)

**出力 (既にセットアップ済みの場合):**

```json
{
  "task_id": "a1b2c3d4",
  "project_id": "myproj",
  "tmux_session": "ma-myproj-a1b2c3",
  "agent_launched": true
}
```

### delete — タスク削除

```bash
ma-task delete <task-id>
```

タスクとそれに紐づくリソースを削除する。以下の順序でクリーンアップを行う:

1. tmux セッションの終了 (`ma-` プレフィックス検証付き)
2. git worktree の削除 (`task/` プレフィックスのブランチのみ削除)
3. `~/.claude.json` の信頼済みディレクトリエントリの削除
4. タスクディレクトリの削除

クリーンアップ中の警告は JSON 出力の `warnings` フィールドに集約される。警告が発生した場合は exit code `1` で終了する。

**出力:**

```json
{
  "deleted": true,
  "task_id": "a1b2c3d4",
  "project_id": "myproj",
  "warnings": []
}
```

### projects — プロジェクト一覧

```bash
ma-task projects
```

全プロジェクトの `id`, `name`, `description` を JSON 配列で出力する。

**出力例:**

```json
[
  {
    "id": "myproj",
    "name": "myproj",
    "description": "プロジェクトの説明"
  }
]
```

### help — ヘルプ表示

```bash
ma-task help
```

## task.json スキーマ

```json
{
  "id": "a1b2c3d4",
  "project_id": "myproj",
  "name": "Fix login timeout",
  "priority": "P2",
  "status": "Todo",
  "agent_cli": "Claude",
  "worktrees": [],
  "links": [],
  "notes": null,
  "initial_instructions": "auth.rs のログインタイムアウト問題を修正してください",
  "tmux_session": null,
  "created_at": "2026-03-11T00:00:00Z",
  "updated_at": "2026-03-11T00:00:00Z"
}
```

| フィールド | 型 | 説明 |
|---|---|---|
| `id` | string | 8文字の hex ID |
| `project_id` | string | 所属プロジェクト ID |
| `name` | string | タスク名 |
| `priority` | string | `P1`-`P5` |
| `status` | string | `Todo` `InProgress` `ActionRequired` `Completed` `Blocked` |
| `agent_cli` | string | `Claude` `Codex` `None` |
| `worktrees` | array | `--run` 実行後にセットアップされる worktree 情報 |
| `links` | array | `{url, display_name}` のリスト |
| `notes` | string\|null | メモ |
| `initial_instructions` | string\|null | エージェントへの初期プロンプト |
| `tmux_session` | string\|null | `--run` 実行後にセットされる tmux セッション名 |
| `created_at` | string | ISO 8601 UTC |
| `updated_at` | string | ISO 8601 UTC |

## エラーハンドリング

- すべてのエラーは stderr に `error: <message>` 形式で出力される
- エラー時の exit code は `1`
- 成功時の exit code は `0`
- `--run` 使用時、`my-agents setup-task` の exit code がそのまま伝搬される

## タスク ID の解決

タスク ID は以下の優先順位で解決される:

1. CWD がタスクディレクトリ内 → そのプロジェクト内で完全一致検索
2. 全プロジェクトを横断して前方一致検索
3. 複数マッチ → `ambiguous task ID` エラー
4. マッチなし → `task not found` エラー
