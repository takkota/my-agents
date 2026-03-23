#!/usr/bin/env bash
# Cursor hooks: log hook_event_name + payload line to CURSOR_HOOK_VERIFY_LOG, then print valid JSON stdout.
# Used by scripts/cursor-hook-verify to test which events fire under `agent --print`.

set -euo pipefail

LOG="${CURSOR_HOOK_VERIFY_LOG:-}"
if [[ -z "$LOG" ]]; then
  echo "CURSOR_HOOK_VERIFY_LOG is not set" >&2
  exit 1
fi

input=$(cat || true)
event=$(echo "$input" | jq -r '.hook_event_name // "unknown"' 2>/dev/null || echo "jq_error")
ts=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
compact=$(echo "$input" | jq -c . 2>/dev/null || echo "{\"raw\":\"$(echo "$input" | head -c 200 | sed 's/"/\\"/g')\"}")
printf '%s\t%s\t%s\n' "$ts" "$event" "$compact" >>"$LOG"

# Responses must match Cursor hook contracts (see https://cursor.com/docs/hooks.md).
case "$event" in
  preToolUse)
    echo '{"permission":"allow"}'
    ;;
  beforeShellExecution | afterShellExecution | beforeMCPExecution | afterMCPExecution)
    echo '{"continue":true,"permission":"allow"}'
    ;;
  beforeReadFile)
    echo '{"permission":"allow"}'
    ;;
  subagentStart)
    echo '{"permission":"allow"}'
    ;;
  beforeSubmitPrompt)
    echo '{"continue":true}'
    ;;
  *)
    echo '{}'
    ;;
esac
