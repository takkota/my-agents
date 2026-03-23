#!/usr/bin/env bash
# Run Cursor Agent in headless mode against a throwaway workspace and record hook events.
# Prerequisites: `agent` on PATH, `jq`, authenticated Cursor account (`agent status`).
#
# Usage:
#   ./run-verify.sh
#   CURSOR_HOOK_VERIFY_LOG=/tmp/hooks.log ./run-verify.sh
#
# After run, inspect the log: unique hook_event_name values indicate which hooks fired.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
VERIFY_DIR="${CURSOR_HOOK_VERIFY_DIR:-$(mktemp -d /tmp/cursor-hook-verify.XXXXXX)}"
LOG="${CURSOR_HOOK_VERIFY_LOG:-$VERIFY_DIR/hook-events.log}"

export CURSOR_HOOK_VERIFY_LOG="$LOG"

mkdir -p "$VERIFY_DIR/.cursor/hooks"
cp "$ROOT/scripts/cursor-hook-verify/hooks.json" "$VERIFY_DIR/.cursor/hooks.json"
cp "$ROOT/scripts/cursor-hook-verify/log-hook.sh" "$VERIFY_DIR/.cursor/hooks/log-hook.sh"
chmod +x "$VERIFY_DIR/.cursor/hooks/log-hook.sh"

: >"$LOG"

echo "Workspace: $VERIFY_DIR"
echo "Hook log:  $LOG"
echo ""

if ! command -v agent >/dev/null 2>&1; then
  echo "error: agent (Cursor CLI) not found on PATH" >&2
  exit 1
fi
if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq not found on PATH" >&2
  exit 1
fi

# Minimal prompt: answer without tools when possible (still may trigger model-dependent hooks).
PROMPT="Reply with exactly the single word: OK"

set +e
agent \
  --workspace "$VERIFY_DIR" \
  --trust \
  --model composer-2-fast \
  --yolo \
  --print \
  -p "$PROMPT"
code=$?
set -e

echo ""
echo "agent exit code: $code"
echo ""
echo "---- Hook events (unique hook_event_name) ----"
if [[ -s "$LOG" ]]; then
  cut -f2 "$LOG" | sort -u
else
  echo "(no hook lines written — hooks did not run or logging failed)"
fi

echo ""
echo "Full log: $LOG"
