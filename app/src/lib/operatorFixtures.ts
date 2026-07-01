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
    parsed: name === 'status' ? fixture.localStatus : fixture[name] ?? null,
    text: name === 'sessions' ? fixture.sessionsText : ''
  };
}

export function buildFixtureOverview(force = false) {
  const storage = browserStorage();
  if (storage && !force) {
    const saved = storage.getItem(FIXTURE_OVERVIEW_KEY);
    if (saved) {
      try {
        const parsed = JSON.parse(saved);
        if (parsed.fixtureVersion === 2) {
          return { ...parsed, generatedAt: new Date().toISOString() };
        }
      } catch (_) {
      }
      storage.removeItem(FIXTURE_OVERVIEW_KEY);
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
  const minutesAgo = (minutes: number) => new Date(Date.now() - minutes * 60 * 1000).toISOString();
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
    fixtureVersion: 2,
    generatedAt: now,
    workflowPath,
    targetContext: {
      workflowPath,
      repository: 'Alive24/shea-symphony',
      workspacePath: null,
      skillsPath: '.codex/skills',
      mode: 'self',
      selfWorkspace: true,
      readiness: { status: 'ready', blockers: [] }
    },
    fixture: true,
    commands: {
      autopilot: fixtureCommand(['autopilot', 'plan', workflowPath, '--json']),
      doctor: fixtureCommand(['doctor', workflowPath, '--json']),
      review: fixtureCommand(['review', 'status', workflowPath, '--json']),
      skills: fixtureCommand(['skills', 'status', workflowPath, '--json']),
      sessions: fixtureCommand(['session', 'list', workflowPath]),
      status: fixtureCommand(['status', 'show', workflowPath, '--json']),
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
      worktreeCount: 3,
      buildPresent: true,
      binaryPresent: true,
      dirtyPreview: [],
      issueWorktrees: [
        {
          issue: '#409',
          title: 'Tauri read surface should preserve doctor refresh state',
          state: 'Done',
          lane: 'Merge',
          path: '/tmp/shea-symphony/worktrees/issue-409-doctor-refresh',
          branch: 'feature/issue-409-doctor-refresh',
          head: 'fixture409',
          lastModified: minutesAgo(34),
          evidence: 'fixture local worktree'
        },
        {
          issue: '#412',
          title: 'Operator desk menu consolidation follow-up',
          state: 'Done',
          lane: 'Merge',
          path: '/tmp/shea-symphony/worktrees/issue-412-menu-followup',
          branch: 'feature/issue-412-menu-followup',
          head: 'fixture412',
          lastModified: minutesAgo(164),
          evidence: 'fixture local worktree'
        },
        {
          issue: '#417',
          title: 'Lane detail timeline should link back to tracker evidence',
          state: 'Done',
          lane: 'Review',
          path: '/tmp/shea-symphony/worktrees/issue-417-lane-detail',
          branch: 'feature/issue-417-lane-detail',
          head: 'fixture417',
          lastModified: minutesAgo(490),
          evidence: 'fixture local worktree'
        }
      ],
      completedIssueWorktrees: [
        {
          issue: '#409',
          title: 'Tauri read surface should preserve doctor refresh state',
          state: 'Done',
          lane: 'Merge',
          url: 'https://github.com/Alive24/shea-symphony/issues/409',
          path: '/tmp/shea-symphony/worktrees/issue-409-doctor-refresh',
          branch: 'feature/issue-409-doctor-refresh',
          head: 'fixture409',
          createdAt: minutesAgo(950),
          lastProgressAt: minutesAgo(34),
          lastModified: minutesAgo(29),
          treeState: 'clean',
          completedAt: minutesAgo(34)
        },
        {
          issue: '#412',
          title: 'Operator desk menu consolidation follow-up',
          state: 'Done',
          lane: 'Merge',
          url: 'https://github.com/Alive24/shea-symphony/issues/412',
          path: '/tmp/shea-symphony/worktrees/issue-412-menu-followup',
          branch: 'feature/issue-412-menu-followup',
          head: 'fixture412',
          createdAt: minutesAgo(620),
          lastProgressAt: minutesAgo(164),
          lastModified: minutesAgo(151),
          treeState: 'dirty',
          completedAt: minutesAgo(164)
        },
        {
          issue: '#417',
          title: 'Lane detail timeline should link back to tracker evidence',
          state: 'Done',
          lane: 'Review',
          url: 'https://github.com/Alive24/shea-symphony/issues/417',
          path: '/tmp/shea-symphony/worktrees/issue-417-lane-detail',
          branch: 'feature/issue-417-lane-detail',
          head: 'fixture417',
          createdAt: minutesAgo(980),
          lastProgressAt: minutesAgo(490),
          lastModified: minutesAgo(486),
          treeState: 'clean',
          completedAt: minutesAgo(490)
        }
      ],
      issueLifecycle: {
        '#409': [
          { phase: 'Backlog', label: 'Created in Project Backlog', time: minutesAgo(950), url: 'https://github.com/Alive24/shea-symphony/issues/409' },
          { phase: 'Promoted', label: 'Promoted to Todo', time: minutesAgo(890), url: 'https://github.com/Alive24/shea-symphony/issues/409#issuecomment-1004091' },
          { phase: 'Main', label: 'Main lane picked up implementation', time: minutesAgo(740), url: 'https://github.com/Alive24/shea-symphony/issues/409#issuecomment-1004092' },
          { phase: 'Agent Review', label: 'Independent review recorded pass evidence', time: minutesAgo(210), url: 'https://github.com/Alive24/shea-symphony/issues/409#issuecomment-1004093' },
          { phase: 'Human Review', label: 'Operator approved merge routing', time: minutesAgo(86), url: 'https://github.com/Alive24/shea-symphony/issues/409#issuecomment-1004094' },
          { phase: 'Done', label: 'Merge lane completed closeout', time: minutesAgo(34), url: 'https://github.com/Alive24/shea-symphony/issues/409#issuecomment-1004095' }
        ],
        '#412': [
          { phase: 'Backlog', label: 'Created in Project Backlog', time: minutesAgo(620), url: 'https://github.com/Alive24/shea-symphony/issues/412' },
          { phase: 'Main', label: 'Main lane wrote menu changes', time: minutesAgo(470), url: 'https://github.com/Alive24/shea-symphony/issues/412#issuecomment-1004121' },
          { phase: 'Rework', label: 'Review returned compactness fix', time: minutesAgo(330), url: 'https://github.com/Alive24/shea-symphony/issues/412#issuecomment-1004122' },
          { phase: 'Done', label: 'Merge lane closed issue', time: minutesAgo(164), url: 'https://github.com/Alive24/shea-symphony/issues/412#issuecomment-1004123' }
        ],
        '#417': [
          { phase: 'Backlog', label: 'Created from LaneIssueView polish note', time: minutesAgo(980), url: 'https://github.com/Alive24/shea-symphony/issues/417' },
          { phase: 'Main', label: 'Implementation handoff published', time: minutesAgo(790), url: 'https://github.com/Alive24/shea-symphony/issues/417#issuecomment-1004171' },
          { phase: 'Agent Review', label: 'Review evidence linked tracker timeline', time: minutesAgo(610), url: 'https://github.com/Alive24/shea-symphony/issues/417#issuecomment-1004172' },
          { phase: 'Done', label: 'Closeout retained local worktree', time: minutesAgo(490), url: 'https://github.com/Alive24/shea-symphony/issues/417#issuecomment-1004173' }
        ]
      }
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
