import test from 'node:test';
import assert from 'node:assert/strict';

import { buildHealth, buildOverview, buildReadSurface, commandArgsFor, createSheaWebServer } from '../server.mjs';
import { buildViewModel, mergeReadSurface } from '../src/lib/api.ts';

test('maps read-only overview commands to allowlisted CLI args', async () => {
  assert.deepEqual(await commandArgsFor({ action: 'autopilot-plan' }), [
    'autopilot',
    'plan',
    'workflows/shea-symphony.md',
    '--json'
  ]);
  assert.deepEqual(await commandArgsFor({ action: 'doctor' }), [
    'doctor',
    'workflows/shea-symphony.md',
    '--json'
  ]);
});

test('normalizes issue refs and keeps project reads read-only', async () => {
  assert.deepEqual(await commandArgsFor({ action: 'project-issue', issue: '418' }), [
    'project',
    'issue',
    'workflows/shea-symphony.md',
    '#418',
    '--json'
  ]);
});

test('supports read-only operator surfaces beyond the overview', async () => {
  assert.deepEqual(await commandArgsFor({ action: 'session-list' }), [
    'session',
    'list',
    'workflows/shea-symphony.md'
  ]);
  assert.deepEqual(await commandArgsFor({ action: 'workspace-list' }), [
    'workspace',
    'list',
    'workflows/shea-symphony.md'
  ]);
  assert.deepEqual(await commandArgsFor({ action: 'clean-audit' }), [
    'clean',
    'audit',
    'workflows/shea-symphony.md'
  ]);
});

test('writes review evidence files for manual review routing commands', async () => {
  const passArgs = await commandArgsFor({
    action: 'review-pass',
    issue: '#421',
    markdown: 'review evidence',
    write: false
  });
  assert.deepEqual(passArgs.slice(0, 5), ['review', 'pass', 'workflows/shea-symphony.md', '#421', '--evidence-file']);
  assert.equal(passArgs.at(-1), '--dry-run');

  const rejectArgs = await commandArgsFor({
    action: 'review-reject',
    issue: '#421',
    markdown: 'finding evidence',
    targetState: 'rework',
    write: true
  });
  assert.deepEqual(rejectArgs.slice(0, 5), ['review', 'reject', 'workflows/shea-symphony.md', '#421', '--evidence-file']);
  assert.deepEqual(rejectArgs.slice(-3), ['--target-state', 'rework', '--write']);
});

test('requires markdown for evidence-writing commands', async () => {
  await assert.rejects(
    () => commandArgsFor({ action: 'review-pass', issue: '#421', markdown: '' }),
    /review evidence markdown/
  );
  await assert.rejects(
    () => commandArgsFor({ action: 'review-reject', issue: '#421', markdown: 'x', targetState: 'Human Review' }),
    /review reject target/
  );
});

test('maps doctor repair actions without writing by default', async () => {
  assert.deepEqual(await commandArgsFor({ action: 'doctor-repair', issue: '#421' }), [
    'doctor',
    'repair',
    '421',
    '--dry-run'
  ]);
  assert.deepEqual(
    await commandArgsFor({
      action: 'doctor-repair',
      issue: '#421',
      repairAction: 'mark_pr_ready',
      write: true
    }),
    ['doctor', 'repair', '421', '--mark-pr-ready', '--confirm-handoff-ready', '--write']
  );
});

test('maps forge validate and create with temporary body files', async () => {
  assert.deepEqual(await commandArgsFor({ action: 'forge-validate', issue: '#409', forgeStatus: 'Todo' }), [
    'forge',
    'validate',
    '--workflow',
    'workflows/shea-symphony.md',
    '--status',
    'Todo',
    '--issue',
    '#409'
  ]);

  const createArgs = await commandArgsFor({
    action: 'forge-create',
    title: 'Follow-up issue',
    markdown: '## Contract',
    forgeStatus: 'Backlog',
    assignees: 'Alive24',
    write: false
  });
  assert.deepEqual(createArgs.slice(0, 9), [
    'forge',
    'create',
    '--workflow',
    'workflows/shea-symphony.md',
    '--status',
    'Backlog',
    '--title',
    'Follow-up issue',
    '--body-file'
  ]);
  assert.deepEqual(createArgs.slice(-3), ['--assignee', 'Alive24', '--dry-run']);
});

test('rejects invalid forge inputs', async () => {
  await assert.rejects(() => commandArgsFor({ action: 'forge-create', title: '', markdown: 'x' }), /title/);
  await assert.rejects(() => commandArgsFor({ action: 'forge-create', title: 'x', markdown: '' }), /body markdown/);
  await assert.rejects(() => commandArgsFor({ action: 'forge-validate', forgeStatus: 'Done', issue: '#1' }), /Backlog or Todo/);
});

test('uses dry-run for mutating commands until write is explicit', async () => {
  assert.deepEqual(await commandArgsFor({ action: 'set-state', issue: '#418', state: 'Rework' }), [
    'project',
    'set-state',
    'workflows/shea-symphony.md',
    '#418',
    'Rework',
    '--dry-run'
  ]);
  assert.deepEqual(
    await commandArgsFor({ action: 'set-state', issue: '#418', state: 'Rework', write: true }),
    ['project', 'set-state', 'workflows/shea-symphony.md', '#418', 'Rework', '--write']
  );
});

test('rejects unsupported command actions and malformed issue refs', async () => {
  await assert.rejects(() => commandArgsFor({ action: 'shell', issue: '#1' }), /unsupported action/);
  await assert.rejects(() => commandArgsFor({ action: 'project-issue', issue: 'abc' }), /#123/);
});

test('exports a server without binding during import', () => {
  const server = createSheaWebServer();
  assert.equal(typeof server.listen, 'function');
  server.close();
});

test('reports local health without shelling out', () => {
  const health = buildHealth();
  assert.equal(health.ok, true);
  assert.equal(health.workflowPath, 'workflows/shea-symphony.md');
  assert.equal(typeof health.buildPresent, 'boolean');
  assert.ok(['env', 'binary', 'cargo'].includes(health.cli.mode));
  assert.equal(health.server.port, 5173);
  assert.equal(health.server.overviewTimeoutMs, 15000);
});

test('local read surface reports checkout posture without tracker access', async () => {
  const surface = await buildReadSurface('local', true);

  assert.equal(surface.name, 'local');
  assert.equal(surface.command.ok, true);
  assert.equal(typeof surface.parsed.branch, 'string');
  assert.equal(typeof surface.parsed.dirtyCount, 'number');
  assert.equal(typeof surface.parsed.worktreeCount, 'number');
  assert.equal(typeof surface.parsed.buildPresent, 'boolean');
  assert.equal(typeof surface.parsed.binaryPresent, 'boolean');
});

test('fixture mode returns live-shaped operator data without shelling out', async () => {
  process.env.SHEA_WEB_FIXTURE = '1';
  const overview = await buildOverview(true);
  delete process.env.SHEA_WEB_FIXTURE;

  assert.equal(overview.fixture, true);
  assert.equal(overview.healthy, true);
  assert.equal(overview.autopilot.lanes.length, 3);
  assert.equal(overview.autopilot.parked_queues[0].issues[0].identifier, '#421');
});

test('offline fallback does not present fake Project or worker work', () => {
  const view = buildViewModel(null);

  assert.equal(view.dataSource.mode, 'offline');
  assert.equal(view.queueIssues.length, 0);
  assert.equal(view.projectWorkerMatch.projectTotal, 0);
  assert.equal(view.projectWorkerMatch.workerTotal, 0);
  assert.ok(view.attentionTasks.every((task) => task.type === 'Diagnostics'));
  assert.ok(view.laneSummaries.every((lane) => lane.active === 0));
  assert.ok(view.laneSummaries.every((lane) => lane.blocked === 0));
  assert.equal(view.operatorBrief.focus.title, 'Live data unavailable');
});

test('fast overview returns live fast surfaces and defers slow tracker reads', async () => {
  process.env.SHEA_WEB_FIXTURE = '1';
  const fixture = await buildOverview(true, 'fast');
  delete process.env.SHEA_WEB_FIXTURE;
  assert.equal(fixture.fixture, true);

  const view = buildViewModel({
    generatedAt: new Date().toISOString(),
    workflowPath: 'workflows/shea-symphony.md',
    scope: 'fast',
    commands: {
      autopilot: {
        ok: false,
        pending: true,
        args: ['autopilot', 'plan', 'workflows/shea-symphony.md', '--json'],
        exitCode: null,
        signal: null,
        timedOut: false,
        durationMs: 0,
        stderr: 'Deferred to full overview.',
        stdoutPreview: ''
      },
      doctor: {
        ok: false,
        pending: true,
        args: ['doctor', 'workflows/shea-symphony.md', '--json'],
        exitCode: null,
        signal: null,
        timedOut: false,
        durationMs: 0,
        stderr: 'Deferred to full overview.',
        stdoutPreview: ''
      },
      review: {
        ok: false,
        pending: true,
        args: ['review', 'status', 'workflows/shea-symphony.md', '--json'],
        exitCode: null,
        signal: null,
        timedOut: false,
        durationMs: 0,
        stderr: 'Deferred to full overview.',
        stdoutPreview: ''
      },
      skills: {
        ok: true,
        args: ['skills', 'status', 'workflows/shea-symphony.md', '--json'],
        exitCode: 0,
        signal: null,
        timedOut: false,
        durationMs: 42,
        stderr: '',
        stdoutPreview: '{}'
      },
      sessions: {
        ok: true,
        args: ['session', 'list', 'workflows/shea-symphony.md'],
        exitCode: 0,
        signal: null,
        timedOut: false,
        durationMs: 30,
        stderr: '',
        stdoutPreview: 'agent_session_list=none'
      },
      local: {
        ok: true,
        args: ['local', 'status'],
        exitCode: 0,
        signal: null,
        timedOut: false,
        durationMs: 5,
        stderr: '',
        stdoutPreview: '{}'
      }
    },
    autopilot: null,
    doctor: null,
    review: null,
    skills: {
      summary: {
        expected_skills: 8,
        blockers: 8,
        codex_status: 'missing_required'
      }
    },
    sessionsText: 'agent_session_list=none',
    localStatus: {
      branch: 'main',
      head: 'abc1234',
      dirtyCount: 2,
      worktreeCount: 1,
      buildPresent: true,
      binaryPresent: true
    },
    healthy: true
  });
  assert.equal(view.dataSource.label, 'Fast live data');
  assert.match(view.dataSource.detail, /pending slow reads/);
  assert.ok(view.laneSummaries.every((lane) => lane.sourceLabel === 'Pending slow read'));
  assert.ok(view.laneSummaries.every((lane) => lane.active === 0));
  assert.ok(view.readinessItems.some((item) => item.label === 'Autopilot plan' && item.status === 'Loading'));
  assert.ok(view.liveSignals.some((signal) => signal.label === 'Workers' && signal.value === '0'));
  assert.ok(view.liveSignals.some((signal) => signal.label === 'Skills' && signal.detail.includes('blocker')));
  assert.ok(view.liveSignals.some((signal) => signal.label === 'Local' && signal.value === 'main'));
  assert.ok(!view.attentionTasks.some((task) => task.id === 'autopilot'));
});

test('read surface payloads can incrementally replace pending overview commands', async () => {
  process.env.SHEA_WEB_FIXTURE = '1';
  const surface = await buildReadSurface('autopilot', true);
  delete process.env.SHEA_WEB_FIXTURE;
  const overview = {
    generatedAt: new Date().toISOString(),
    workflowPath: 'workflows/shea-symphony.md',
    scope: 'fast',
    commands: {
      autopilot: {
        ok: false,
        pending: true,
        args: ['autopilot', 'plan', 'workflows/shea-symphony.md', '--json'],
        exitCode: null,
        signal: null,
        timedOut: false,
        durationMs: 0,
        stderr: 'Deferred to full overview.',
        stdoutPreview: ''
      }
    },
    autopilot: null,
    healthy: false
  };

  assert.equal(surface.name, 'autopilot');
  assert.equal(surface.command.ok, true);
  assert.equal(surface.parsed.lanes.length, 3);

  const merged = mergeReadSurface(overview, surface);
  const view = buildViewModel(merged);
  assert.equal(merged.commands.autopilot.ok, true);
  assert.equal(merged.autopilot.lanes.length, 3);
  assert.ok(view.laneSummaries.some((lane) => lane.sourceLabel === 'Live autopilot'));
  assert.ok(view.liveSignals.some((signal) => signal.label === 'Queue'));
  assert.ok(view.readinessItems.some((item) => item.label === 'Autopilot plan' && item.status === 'Ready'));
});

test('view model derives visualization structures from overview data', async () => {
  process.env.SHEA_WEB_FIXTURE = '1';
  const overview = await buildOverview(true);
  delete process.env.SHEA_WEB_FIXTURE;

  const view = buildViewModel(overview);
  assert.ok(view.stateDistribution.some((row) => row.state === 'Need Human Input' && row.count === 1));
  assert.ok(view.stateDistribution.some((row) => row.state === 'Todo' && row.count === 1));
  assert.ok(view.stateDistribution.some((row) => row.state === 'In Progress' && row.count === 0));
  assert.ok(view.evidenceColumns.some((column) => column.lane === 'System' && column.events.length > 0));
  assert.ok(view.trackerSignals.some((signal) => signal.label === 'ProjectV2 tracker' && signal.status === 'Readable'));
  assert.ok(view.gateChecklist.some((gate) => gate.label === 'Agent Review Gate'));
  assert.ok(view.timelineModel.some((item) => item.lane === 'Main' && item.writer === 'Persistent Workpad'));
  assert.ok(view.capabilityMap.some((item) => item.label === 'Tracker client abstraction'));
  assert.equal(view.dataSource.mode, 'fixture');
  assert.equal(view.dataSource.trust, 'Safe for visual QA, not tracker routing');
  assert.ok(view.laneSummaries.every((lane) => lane.provenance === 'fixture'));
  assert.ok(view.laneSummaries.every((lane) => lane.sourceLabel === 'Fixture autopilot'));
  assert.equal(view.operatorBrief.sourceNote, 'Fixture lane posture');
  assert.ok(view.issueIndex.some((issue) => issue.id === '#421' && issue.sources.includes('attention')));
  assert.ok(view.issueIndex.some((issue) => issue.id === '#418' && issue.sources.includes('lane')));
  assert.equal(view.commandHealth.length, 7);
  assert.ok(view.commandHealth.every((command) => command.status === 'Passed'));
  assert.equal(view.operatorBrief.focus.id, '#421');
  assert.ok(view.operatorBrief.skills.some((skill) => skill.label === 'Human Review' && skill.count === 1));
  assert.ok(view.operatorBrief.lanes.some((lane) => lane.name === 'Main' && lane.pressure >= 1));
  assert.ok(view.operatorBrief.evidence.some((item) => item.lane === 'System' && item.count >= 1));
  assert.ok(view.readPathMap.some((path) => path.id === 'web-api' && path.status === 'Available'));
  assert.ok(view.readPathMap.some((path) => path.id === 'skills' && path.status === 'Passed'));
  assert.ok(view.readPathMap.some((path) => path.id === 'local' && path.status === 'Passed'));
  assert.equal(view.laneWorkers.main[0].issue, '#418');
});

test('view model tolerates numeric issue identifiers from live tracker reads', () => {
  const view = buildViewModel({
    generatedAt: new Date().toISOString(),
    workflowPath: 'workflows/shea-symphony.md',
    commands: {},
    autopilot: {
      lanes: [],
      parked_queues: [
        {
          state: 'Need Human Input',
          issues: [
            {
              identifier: 421,
              title: 'Numeric issue identifier',
              evidence: 'Live tracker returned a numeric identifier.'
            }
          ]
        }
      ]
    },
    healthy: true
  });

  assert.ok(view.issueIndex.some((issue) => issue.id === 421));
  assert.equal(view.operatorBrief.focus.id, 421);
});

test('view model matches session workers to lanes and issues', () => {
  const view = buildViewModel({
    generatedAt: new Date().toISOString(),
    workflowPath: 'workflows/shea-symphony.md',
    commands: {
      sessions: {
        ok: true,
        args: ['session', 'list', 'workflows/shea-symphony.md'],
        exitCode: 0,
        signal: null,
        timedOut: false,
        durationMs: 5,
        stderr: '',
        stdoutPreview: 'agent_session session=shea-main-364-attempt-1-rework attached=1'
      }
    },
    sessionsText:
      'agent_session session=shea-main-364-attempt-1-rework attached=1 attach_command="tmux attach-session -t shea-main-364-attempt-1-rework"',
    healthy: true
  });

  assert.equal(view.laneWorkers.main.length, 1);
  assert.equal(view.laneWorkers.main[0].issue, '#364');
  assert.equal(view.laneWorkers.main[0].lane, 'main');
  assert.equal(view.workerMonitor.totalWorkers, 1);
  assert.equal(view.workerMonitor.primaryWorker.issue, '#364');
  assert.equal(view.workerMonitor.primaryWorker.lane, 'main');
  assert.equal(view.workerMonitor.title, '1 worker visible');
  assert.equal(view.liveSignals.find((signal) => signal.label === 'Workers').value, '1');
});

test('view model summarizes Project queue and worker match posture', () => {
  const view = buildViewModel({
    generatedAt: new Date().toISOString(),
    workflowPath: 'workflows/shea-symphony.md',
    commands: {
      githubQueue: {
        ok: true,
        args: ['gh', 'issue', 'list'],
        exitCode: 0,
        signal: null,
        timedOut: false,
        durationMs: 25,
        stderr: '',
        stdoutPreview: '[]'
      },
      sessions: {
        ok: true,
        args: ['session', 'list', 'workflows/shea-symphony.md'],
        exitCode: 0,
        signal: null,
        timedOut: false,
        durationMs: 5,
        stderr: '',
        stdoutPreview: 'agent_session session=shea-main-364-attempt-1-rework attached=1'
      }
    },
    githubQueue: {
      totalOpen: 1,
      source: 'Live GitHub queue',
      stateCounts: { Rework: 1 },
      laneCounts: { main: 1, review: 0, merge: 0 },
      issues: [
        {
          identifier: '#364',
          title: 'Resume blocker work',
          state: 'Rework',
          url: 'https://github.com/Alive24/shea-symphony/issues/364'
        }
      ]
    },
    sessionsText: 'agent_session session=shea-main-364-attempt-1-rework attached=1',
    healthy: true
  });

  assert.equal(view.projectWorkerMatch.summary, '1/1 matched');
  assert.equal(view.projectWorkerMatch.waiting, 0);
  assert.equal(view.projectWorkerMatch.workerTotal, 1);
  assert.equal(view.workerMonitor.totalWorkers, 1);
  assert.equal(view.workerMonitor.totalProjectItems, 1);
  assert.equal(view.workerMonitor.lanes.find((lane) => lane.lane === 'main').workerCount, 1);
  assert.equal(view.projectWorkerMatch.tone, 'success');
  assert.equal(view.projectWorkerMatch.lanes.find((lane) => lane.lane === 'Main').matched, 1);
  assert.equal(view.queueIssues[0].workerStatus, 'Worker matched');
  assert.equal(view.queueIssues[0].nextSkill, 'Manual Main');
  assert.equal(view.currentFocus.id, '#364');
  assert.equal(view.currentFocus.nextSkill, 'Manual Main');
});

test('queue issues explain Project items waiting without a worker', () => {
  const view = buildViewModel({
    generatedAt: new Date().toISOString(),
    workflowPath: 'workflows/shea-symphony.md',
    commands: {
      githubQueue: {
        ok: true,
        args: ['gh', 'issue', 'list'],
        exitCode: 0,
        signal: null,
        timedOut: false,
        durationMs: 25,
        stderr: '',
        stdoutPreview: '[]'
      },
      sessions: {
        ok: true,
        args: ['session', 'list', 'workflows/shea-symphony.md'],
        exitCode: 0,
        signal: null,
        timedOut: false,
        durationMs: 5,
        stderr: '',
        stdoutPreview: 'agent_session_list=none'
      }
    },
    githubQueue: {
      totalOpen: 1,
      source: 'Live GitHub queue',
      stateCounts: { Rework: 1 },
      laneCounts: { main: 1, review: 0, merge: 0 },
      issues: [
        {
          identifier: '#364',
          title: 'Resume blocker work',
          state: 'Rework'
        }
      ]
    },
    sessionsText: 'agent_session_list=none',
    healthy: true
  });

  assert.equal(view.queueIssues[0].workerStatus, 'No worker visible');
  assert.match(view.queueIssues[0].workerDetail, /waiting/);
  assert.equal(view.queueIssues[0].workerTone, 'warn');
  assert.equal(view.projectWorkerMatch.waiting, 1);
  assert.equal(view.workerMonitor.totalWorkers, 0);
  assert.equal(view.workerMonitor.totalProjectItems, 1);
  assert.equal(view.workerMonitor.title, 'No worker visible');
  assert.match(view.workerMonitor.detail, /Project item/);
  assert.equal(view.currentFocus.id, '#364');
  assert.equal(view.currentFocus.tone, 'warn');
  assert.match(view.currentFocus.detail, /waiting/);
  assert.ok(view.stateDistribution.some((row) => row.state === 'Rework' && row.count === 1));
  assert.ok(view.stateDistribution.some((row) => row.state === 'In Progress' && row.count === 0));
});

test('view model does not claim missing workers when session surface is unavailable', () => {
  const view = buildViewModel({
    generatedAt: new Date().toISOString(),
    workflowPath: 'workflows/shea-symphony.md',
    commands: {
      githubQueue: {
        ok: true,
        args: ['gh', 'issue', 'list'],
        exitCode: 0,
        signal: null,
        timedOut: false,
        durationMs: 25,
        stderr: '',
        stdoutPreview: '[]'
      },
      sessions: {
        ok: true,
        args: ['session', 'list', 'workflows/shea-symphony.md'],
        exitCode: 0,
        signal: null,
        timedOut: false,
        durationMs: 5,
        stderr: '',
        stdoutPreview: 'agent_session_list=unavailable reason=tmux_not_executable'
      }
    },
    githubQueue: {
      totalOpen: 1,
      source: 'Live GitHub queue',
      stateCounts: { Rework: 1 },
      laneCounts: { main: 1, review: 0, merge: 0 },
      issues: [
        {
          identifier: '#364',
          title: 'Resume blocker work',
          state: 'Rework'
        }
      ]
    },
    sessionsText: 'agent_session_list=unavailable reason=tmux_not_executable',
    healthy: true
  });

  assert.equal(view.sessionReadState.status, 'unavailable');
  assert.equal(view.projectWorkerMatch.label, 'Worker read unavailable');
  assert.equal(view.queueIssues[0].workerStatus, 'Worker read unavailable');
  assert.ok(view.liveSignals.some((signal) => signal.label === 'Workers' && signal.value === '!'));
});

test('view model explains timed out overview commands for observation', () => {
  const view = buildViewModel({
    generatedAt: new Date().toISOString(),
    workflowPath: 'workflows/shea-symphony.md',
    commands: {
      autopilot: {
        ok: false,
        args: ['autopilot', 'plan', 'workflows/shea-symphony.md', '--json'],
        exitCode: null,
        signal: 'SIGTERM',
        timedOut: true,
        durationMs: 15005,
        stderr: '',
        stdoutPreview: ''
      }
    },
    autopilot: null,
    healthy: false
  });

  const autopilot = view.commandHealth.find((command) => command.id === 'autopilot');
  assert.equal(autopilot.detail, 'Timed out after 15s.');
  assert.equal(autopilot.exit, 'timeout / SIGTERM');
  assert.match(autopilot.impact, /Lane counts/);
  assert.match(autopilot.recommendation, /slow read surface/);
  assert.ok(view.laneSummaries.every((lane) => lane.provenance === 'partial'));
  assert.ok(view.laneSummaries.every((lane) => lane.sourceLabel === 'Timed-out fallback'));
  assert.ok(view.laneSummaries.every((lane) => lane.active === 0));
  assert.ok(view.stateDistribution.some((row) => row.state === 'In Progress' && row.count === 0));
  assert.equal(view.operatorBrief.sourceNote, 'Lane counts are partial');
  assert.ok(view.readPathMap.some((path) => path.id === 'tracker' && path.status === 'Failed' && path.tone === 'warn'));
  assert.ok(view.attentionTasks.some((task) => task.id === 'autopilot' && task.reason === 'Timed out after 15s.'));
});

test('view model does not inflate empty live lanes with fallback counts', () => {
  const view = buildViewModel({
    generatedAt: new Date().toISOString(),
    workflowPath: 'workflows/shea-symphony.md',
    commands: {
      autopilot: {
        ok: true,
        args: ['autopilot', 'plan', 'workflows/shea-symphony.md', '--json'],
        exitCode: 0,
        signal: null,
        timedOut: false,
        durationMs: 92,
        stderr: '',
        stdoutPreview: '{"lanes":[]}'
      }
    },
    autopilot: {
      lanes: [],
      parked_queues: [],
      lane_activity: []
    },
    healthy: true
  });

  assert.ok(view.laneSummaries.every((lane) => lane.provenance === 'live'));
  assert.ok(view.laneSummaries.every((lane) => lane.sourceLabel === 'Live empty lane'));
  assert.ok(view.laneSummaries.every((lane) => lane.active === 0));
  assert.equal(view.operatorBrief.sourceNote, 'Lane counts from live reads');
});
