import { browserStorage, FIXTURE_OVERVIEW_KEY } from './uiState.ts';

export function buildFixtureReadSurface(name: string, force = false) {
  const fixture = buildFixtureOverview(force);
  const command = fixture.commands?.[name] ?? {
    ok: true,
    args: ['fixture', name],
    exitCode: 0,
    durationMs: 8,
    stderr: '',
    stdoutPreview: 'fixture output'
  };
  return {
    name,
    generatedAt: fixture.generatedAt,
    command,
    parsed: name === 'local' ? fixture.localStatus : fixture[name] ?? null,
    text: name === 'sessions' ? fixture.sessionsText : ''
  };
}

export function buildFixtureOverview(force = false) {
  const storage = browserStorage();
  if (storage && !force) {
    const saved = storage.getItem(FIXTURE_OVERVIEW_KEY);
    if (saved) {
      try {
        return { ...JSON.parse(saved), generatedAt: new Date().toISOString() };
      } catch (_) {
        storage.removeItem(FIXTURE_OVERVIEW_KEY);
      }
    }
  }

  const overview = baseFixtureOverview();
  if (storage) {
    storage.setItem(FIXTURE_OVERVIEW_KEY, JSON.stringify(overview));
  }
  return overview;
}


function fixtureCommand(args: string[]) {
  return {
    ok: true,
    args,
    exitCode: 0,
    signal: null,
    durationMs: 12,
    stderr: '',
    stdoutPreview: 'fixture output'
  };
}

function baseFixtureOverview() {
  const now = new Date().toISOString();
  const workflowPath = 'workflows/shea-symphony.md';
  const issues = [
    {
      identifier: '#418',
      number: 418,
      title: 'Forge contract needs blocker relationship clarification',
      state: 'Need to Clarify',
      lane: 'Human',
      updatedAt: now,
      url: 'https://github.com/Alive24/shea-symphony/issues/418',
      labels: ['fixture', 'issue-forge'],
      assignees: ['operator']
    },
    {
      identifier: '#421',
      number: 421,
      title: 'Agent Review evidence needs Human Review routing',
      state: 'Need Human Input',
      lane: 'Human',
      updatedAt: now,
      url: 'https://github.com/Alive24/shea-symphony/issues/421',
      labels: ['fixture', 'review'],
      assignees: ['operator']
    },
    {
      identifier: '#425',
      number: 425,
      title: 'Parent app-server batch awaits UAT approval',
      state: 'Human Review',
      lane: 'Human',
      updatedAt: now,
      url: 'https://github.com/Alive24/shea-symphony/issues/425',
      labels: ['fixture', 'human-review'],
      assignees: ['operator']
    },
    {
      identifier: '#430',
      number: 430,
      title: 'Merge lane should land approved app-server cleanup',
      state: 'Merging',
      lane: 'Merge',
      updatedAt: now,
      url: 'https://github.com/Alive24/shea-symphony/issues/430',
      labels: ['fixture', 'merge'],
      assignees: ['merge-agent']
    }
  ];

  return {
    generatedAt: now,
    workflowPath,
    fixture: true,
    commands: {
      autopilot: fixtureCommand(['autopilot', 'plan', workflowPath, '--json']),
      doctor: fixtureCommand(['doctor', workflowPath, '--json']),
      review: fixtureCommand(['review', 'status', workflowPath, '--json']),
      skills: fixtureCommand(['skills', 'status', workflowPath, '--json']),
      sessions: fixtureCommand(['session', 'list', workflowPath]),
      local: fixtureCommand(['local', 'status']),
      githubQueue: fixtureCommand(['autopilot', 'plan', workflowPath, '--json'])
    },
    autopilot: {
      readiness: {
        status: 'ready',
        reason: 'Fixture mode is exercising the operator UI without GitHub writes.'
      },
      lanes: [
        {
          lane: 'main',
          status: 'ready',
          selected_issue: '#418',
          action: 'Clarify blocker relationship semantics',
          reason: 'Issue needs one execution-critical product decision.',
          target_state: 'Need to Clarify'
        },
        {
          lane: 'review',
          status: 'blocked',
          selected_issue: '#421',
          action: 'Human review routing decision needed',
          reason: 'Independent review evidence is present; operator must choose pass or reject.',
          target_state: 'Human Review'
        },
        {
          lane: 'merge',
          status: 'ready',
          selected_issue: '#430',
          action: 'Verify approved PR and land',
          reason: 'Fixture merge queue has one approved issue.',
          target_state: 'Done'
        }
      ],
      parked_queues: [
        {
          state: 'Need Human Input',
          reason: 'Operator confirmation required before status mutation.',
          issues: [
            {
              identifier: '#421',
              title: 'Agent Review evidence needs Human Review routing',
              reason: 'Review evidence exists but no human decision has been recorded.',
              evidence: 'Fixture: review pass checklist is present; PR readback is fresh.'
            }
          ]
        }
      ],
      active_issues: []
    },
    doctor: { blockers: 0, warnings: 1 },
    review: { recent: [{ issue: '#421', state: 'passed', evidence: 'Fixture review pass evidence.' }] },
    skills: { status: 'ready' },
    sessionsText: 'agent_session_list=none',
    localStatus: {
      branch: 'main',
      head: 'fixture',
      dirtyCount: 0,
      worktreeCount: 1,
      buildPresent: true,
      binaryPresent: true,
      dirtyPreview: []
    },
    githubQueue: {
      totalOpen: issues.length,
      stateCounts: {
        'Need to Clarify': 1,
        'Need Human Input': 1,
        'Human Review': 1,
        Merging: 1
      },
      laneCounts: {
        main: 0,
        review: 0,
        merge: 1
      },
      operatorIssues: issues.filter((issue) => ['Need to Clarify', 'Need Human Input', 'Human Review'].includes(issue.state)),
      issues,
      source: 'fixture operator queue'
    },
    healthy: true
  };
}
