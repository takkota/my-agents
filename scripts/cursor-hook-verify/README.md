# Cursor CLI フック検証（`agent --print`）

my-agents で Cursor 向けに `stop` / `beforeSubmitPrompt` / `postToolUse` などを使う**前に**、ローカルの Cursor Agent が実際にフックを発火するか確認するための補助ファイルです。

## 前提

- [Cursor CLI](https://cursor.com/docs) の `agent` が PATH にあること
- `jq`
- Cursor にログイン済み（`agent status` で確認）

## 手順

リポジトリルートから:

```bash
chmod +x scripts/cursor-hook-verify/run-verify.sh scripts/cursor-hook-verify/log-hook.sh
./scripts/cursor-hook-verify/run-verify.sh
```

一時ディレクトリとログの場所を固定したい場合:

```bash
export CURSOR_HOOK_VERIFY_DIR=/tmp/my-cursor-hook-test
mkdir -p "$CURSOR_HOOK_VERIFY_DIR"
export CURSOR_HOOK_VERIFY_LOG="$CURSOR_HOOK_VERIFY_DIR/hook-events.log"
./scripts/cursor-hook-verify/run-verify.sh
```

## 結果の読み方

実行後、標準出力に **ユニークな `hook_event_name` 一覧**が出ます。ここに例えば `stop` や `beforeSubmitPrompt` が**無い**場合、その Cursor バージョンの headless (`--print`) 経路では当該フックは使えないと判断し、**my-agents の `write_cursor_hooks` を拡張しない**（別手段を検討する）運用にしてください。

`hook-events.log` には各発火のタイムスタンプと JSON 1 行が残ります。

## my-agents 開発ポリシー

- **フックに依存する Cursor 向け機能を追加・変更する前に**、上記スクリプト（または同等の手動検証）で対象イベントが発火することを確認する。
- Cursor のアップデート後は再実行し、挙動が変わっていないか確認する。

## 実測メモ（参考）

次の環境で `run-verify.sh` およびツール利用プロンプトを試した結果の例です。バージョンが変われば必ず再実行してください。

| Cursor agent ビルド | `agent --print --trust` で観測されたイベント |
|---------------------|---------------------------------------------|
| `2026.03.20-44cb435` | `sessionStart`, `sessionEnd`, `preToolUse`, `postToolUse`, `beforeReadFile`, `beforeShellExecution`, `afterShellExecution` |
| 同上 | **`stop`, `beforeSubmitPrompt` は発火せず** |

## 参考

- [Hooks](https://cursor.com/docs/hooks.md)
- [プラグイン（フック一覧）](https://cursor.com/ja/docs/reference/plugins)
