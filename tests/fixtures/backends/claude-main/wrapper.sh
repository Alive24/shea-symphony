#!/bin/sh
set -eu

# Deterministic protocol fixture: accepts Shea's official stream-json flags and input.
cat >/dev/null
cat "$(dirname "$0")/success.jsonl"
