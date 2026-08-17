#!/bin/sh
set -eu

# Accept Shea's one stream-json input record without exposing it in output.
IFS= read -r _input

case "${SHEA_CLAUDE_REVIEW_FIXTURE:-pass}" in
  finding) fixture=finding.jsonl ;;
  *) fixture=pass.jsonl ;;
esac

session_id="claude-review-$$"
sed "s/claude-review-fixture/$session_id/g" "$(dirname "$0")/$fixture"
