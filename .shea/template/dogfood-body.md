# Dogfood Session Body Contract

This template guides a Dogfood skill when it creates or updates a durable Dogfood session issue for any target project. It is project-agnostic and should not assume a particular workflow topology such as implementation/review/merge lanes.

The skill owns rendering, refreshing, and maintaining this body. Operators add feedback through the skill-guided flow or through clearly marked findings that the skill can fold back into the body.

## Rendering Rules

- Keep the issue body as the session's compact source of truth.
- Preserve completed checkboxes when refreshing live snapshots.
- Refresh live facts through the configured workflow CLI, application tooling, test commands, or tracker-safe project surfaces.
- Do not turn the Dogfood issue into execution authority. It records observations and follow-up pointers; the target project's normal execution, review, approval, release, support, or incident flows keep their own authority boundaries.
- Keep Dogfood session items non-dispatchable by automation unless the target project explicitly defines a safe dogfood automation path.
- Prefer links to issues, PRs, artifacts, logs, screenshots, recordings, and commands over pasted log dumps.
- If a section has no data yet, render `Not observed yet` rather than removing the section.
- Render project-specific phases only when they apply. Examples include `Build`, `CLI`, `UI`, `Review`, `Deploy`, `Incident Response`, or `Implementation / Review / Merge` for projects that use that topology.

## Body Shape

### Dogfood Session

- Session date: `[YYYY-MM-DD]`
- Operator: `[name or handle]`
- Target project: `[project, repo, product, integration, or workflow under test]`
- Dogfood tool: `[installed CLI, skill, app, or manual process name and version]`
- Operator workspace: `[target checkout, app workspace, staging environment, or other working context]`
- Tracker: `[GitHub Issues | GitHub Project | Linear | Jira | other]`
- Session status: `Planned | Running | Paused | Closed`
- Dogfood scope: `[what this session is trying to validate]`

### Snapshot

- Target state: `[current product/project/workflow state]`
- Health check: `[blocker/warning/readiness summary]`
- Workspace or environment: `[branch, version, deployment, staging/prod, cleanliness, or readiness]`
- Candidate work under test: `[issues, PRs, builds, features, flows, or integrations]`
- Known constraints: `[quota, access, environment, model, data, policy, or timing constraints]`

### Recommended Pre-Dogfood Work

The skill should recommend a small number of pre-dogfood items. The goal is to remove obvious friction before the dogfood run, not to create a new development cycle.

- [ ] `[item or issue]` - [Title]
  - Reason: [why this matters before dogfood]
  - Suggested mode: `manual | automated | defer | needs discussion`

- [ ] `[item or issue]` - [Title]
  - Reason: [why this matters before dogfood]
  - Suggested mode: `manual | automated | defer | needs discussion`

### Recently Completed Capabilities

The skill should list only capabilities that are relevant to this dogfood session. Keep each item short and evidence-linked.

- [x] `[item or issue]` - [capability summary]
- [x] `[item or issue]` - [capability summary]

### Dogfood Goals

The skill should render concrete goals for this session. Do not use this section to claim broad readiness unless that is the explicit test.

- [ ] [goal]
- [ ] [goal]
- [ ] [goal]

### Suggested Run Order

The skill should render a short ordered plan and explain why the order matters.

1. [Step]
   - Why: [reason]
   - Tool, command, skill, or manual action: `[action]`

2. [Step]
   - Why: [reason]
   - Tool, command, skill, or manual action: `[action]`

3. [Step]
   - Why: [reason]
   - Tool, command, skill, or manual action: `[action]`

### Commands, Skills, Or Manual Actions

The skill should render the current actions for the target project. Do not hardcode framework-specific commands unless the target project uses them.

#### Preflight

```bash
cd [target project or operator workspace]
[show workspace/environment status]
[show tracker/project state]
[show health/readiness check]
[show dry-run, preview, or rehearsal command when available]
```

#### Execution Actions

```bash
[command, skill, app action, or manual step]
```

#### Observation Actions

```bash
[status, logs, dashboard, artifact, or readback command]
```

### Observations

The skill should update observations after each dogfood action. Use phases that fit the target project rather than assuming a fixed lane topology.

#### Phase: [name]

- Status: `Not observed yet | Passed | Failed | Inconclusive`
- Item under test: `[issue, PR, feature, integration, environment, or workflow]`
- Action:
- Evidence:
- Result:
- Operator friction:
- Follow-up:

Success criteria:

- [ ] [criterion]
- [ ] [criterion]
- [ ] [criterion]

#### Phase: [name]

- Status: `Not observed yet | Passed | Failed | Inconclusive`
- Item under test: `[issue, PR, feature, integration, environment, or workflow]`
- Action:
- Evidence:
- Result:
- Operator friction:
- Follow-up:

Success criteria:

- [ ] [criterion]
- [ ] [criterion]
- [ ] [criterion]

### Findings

The skill should convert operator feedback into structured findings. If a finding is already covered by an issue, link it instead of creating duplicate work.

- [ ] [Finding summary]
  - Phase:
  - Item:
  - Action:
  - Expected:
  - Actual:
  - Evidence:
  - Operator friction:
  - Follow-up issue or existing issue:

### High-Value Feedback Categories

The skill should actively watch for these categories during discussion and adapt the wording to the target project:

- The product or workflow behaves differently from the operator's reasonable expectation.
- The user journey has unclear next steps, missing confirmation, confusing status, or too much hidden state.
- The tested feature cannot be completed without undocumented setup, credentials, data, permissions, or environment assumptions.
- The system succeeds technically but leaves the operator unsure whether the result is safe, final, reversible, or ready for the next step.
- Output, evidence, reports, screenshots, logs, or audit trails are missing, duplicated, hard to find, or hard to interpret.
- Errors do not explain cause, impact, recovery options, or whether retrying is safe.
- Documentation, UI copy, CLI output, skill instructions, support guidance, or actual behavior disagree.
- The workflow requires excessive manual inspection, context switching, copy/paste, or low-level debugging for a normal user to proceed.
- The workflow risks touching the wrong account, project, environment, data boundary, branch, deployment, or external system.
- A failure reveals a broader product assumption that should become a follow-up issue rather than a one-off note.

### Follow-Up Mapping

The skill should route follow-up work through the target project's normal issue creation, triage, or planning process rather than making ad hoc mutations.

- Created:
  - `[item or issue]` - [title]
- Promoted or prioritized:
  - `[item or issue]` - [title]
- Existing coverage:
  - `[item or issue]` - [title]
- Deferred:
  - [reason]

### Closeout

- Dogfood enough for this round: `yes | no`
- Overall confidence: `high | medium | low | not tested`
- Phase confidence:
  - `[phase]`: `high | medium | low | not tested`
  - `[phase]`: `high | medium | low | not tested`
- Next bottleneck:
- Follow-up items created, promoted, or deferred:
- Notes for the next dogfood session:
