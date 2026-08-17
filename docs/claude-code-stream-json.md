# Claude Code stream-json transport

Shea Symphony uses Claude Code's non-interactive `stream-json` protocol for
Main execution, independent Review, and semantic Merge-agent repair. This is
lane-level lifecycle parity with the Codex backend; it is not a Claude
implementation of the Codex app-server protocol. Review uses its separate
`ReviewBackend` adapter while reusing this one transport implementation.

## Configuration boundary

```yaml
main_lane:
  backend: claude-code
merge_lane:
  agent_backend: claude-code
claude:
  command: claude
  turn_timeout_ms: 3600000
review_lane:
  backend: claude-code
  # Optional override; otherwise Review uses claude.command.
  claude_command: claude --permission-mode plan
  timeout_ms: 600000
```

`claude.command` is an executable, executable plus base arguments, or an
operator wrapper. Shea appends `-p --input-format stream-json --output-format
stream-json --verbose` and, for a validated recovery, `--resume <session-id>`.
The configured command owns model choice, authentication, gateway routing,
environment, and permission arguments. Prefer an executable wrapper or a base
command such as `env PROFILE=shea claude`; do not add shell pipelines or
redirections because Shea executes the final command with `exec` for reliable
process cleanup.

Shea writes one JSONL user message to stdin and requires an initialization
event followed by an explicit terminal result. Assistant text, tool use, tool
results, usage, errors, and terminal state are normalized into the existing
lane event contract. Raw protocol, stderr, normalized events, prompt, process
ID, Shea run ID, and Claude session ID are stored below the configured artifact
roots. Environment values are passed to the worker but are not copied into
event messages.

A zero exit code without a successful result is not success. Malformed or
truncated JSONL, session-ID changes, error results, timeout, cancellation, and
nonzero exit all fail closed. Timeout cleanup signals the entire Unix child
process group before force-killing its leader.

Main recovery passes `--resume` only when the session registry record matches
the issue ID and identifier, `main` lane, run ID, worktree path, Claude
backend/source, and recorded session ID. Every new Review job starts a fresh
session independent from Main and Merge. One interrupted Review job may bind
only its own initialized session ID to one retry; its output artifact and
backend-neutral ledger record the session, attempts, protocol, stderr,
normalized events, workspace-integrity result, and routing decision. Merge
repair starts a fresh session and retains its own prompt and routing authority.
Clean mechanical merge handling never launches Claude.

`review_lane.claude_command` overrides the shared command so an operator can
apply a stricter read-only wrapper without changing Main or Merge. The command
or wrapper owns Claude permission arguments. Shea additionally snapshots the
Review workspace before and after execution, rejects any mutation, requires a
schema-complete structured report, and never infers pass from exit status,
empty findings, partial text, or missing output.

See the official [Claude Code CLI reference](https://code.claude.com/docs/en/cli-usage)
for the underlying flags and session contract.

## Deterministic fixture

```bash
cargo test agent::tests::claude_code_backend
```

Protocol fixtures under `tests/fixtures/backends/claude-main/*.jsonl` cover
initialization, assistant and tool progress, usage, success, error,
cancellation, malformed JSON, truncation, resume arguments, timeout cleanup,
and process exit. `tests/fixtures/workflows/claude-main.md` shows a
credential-free wrapper configuration.

`tests/fixtures/workflows/claude-review.md` and
`tests/fixtures/backends/claude-review/*.jsonl` exercise the independent Review
adapter with pass and confirmed-finding reports. Focused deterministic Review
coverage is available with:

```bash
cargo test review::claude::tests --lib
```

## Local-worktree-only UAT

The ignored test creates disposable `main` and `merge` directories, asks the
configured Claude command to modify one small file in each, validates the
result, checks structured session/artifact evidence, and deletes the temporary
directories when the test exits:

```bash
SHEA_CLAUDE_UAT_COMMAND='claude' \
  cargo test claude_code_live_local_worktree_uat -- --ignored --nocapture
```

An operator wrapper can be supplied instead, including its own model, gateway,
authentication, environment, and permission flags. The UAT calls the backend
directly: it does not read or mutate GitHub Project state, push a branch, create
a PR, or invoke the Main/Merge tracker controllers. Inspect the printed failure
or the temporary artifact path before rerunning if the command cannot
authenticate or its permission policy prevents the requested local edits.

The ignored Review UAT creates separate clean and seeded-defect repositories,
requires pass and confirmed-finding reports, and verifies both repositories are
byte-for-byte unchanged:

```bash
SHEA_CLAUDE_REVIEW_UAT_COMMAND='claude --permission-mode plan' \
  cargo test claude_review_live_local_read_only_uat -- --ignored --nocapture
```

It calls the Review backend directly and performs no PR comment, push, merge,
issue mutation, or Project mutation.
