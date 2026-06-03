import test from 'node:test';
import assert from 'node:assert/strict';

import {
  buildFixtureOverview,
  buildFixtureReadSurface
} from '../src/lib/operatorFixtures.ts';
import {
  buildViewModel,
} from '../src/lib/operatorViewModel.ts';
import {
  loadHealth,
  loadOverview,
  loadReadSurface
} from '../src/lib/operatorReads.ts';
import {
  mergeReadSurface
} from '../src/lib/operatorReadModel.ts';
import {
  classifyHeartbeat,
  parseCodexTranscriptJsonl,
  transcriptUnavailable
} from '../src/lib/viewModel/codexTranscript.ts';
import {
  defaultBackgroundReadSurfaces,
  projectCooldownReadSurfaces
} from '../src/lib/operatorOverviewStore.ts';
import {
  buildLaneThroughputBoard
} from '../src/lib/viewModel/laneThroughput.ts';
import { appendAutoloopLine, defaultLoopState, laneWorkerFromAutoloop } from '../src/lib/tauriAutoloop.ts';

test('browser fallback uses fixture data instead of a Node API bridge', async () => {
  const overview = await loadOverview(true, 'fast');
  const surface = await loadReadSurface('autopilot', true);
  const health = await loadHealth();

  assert.equal(overview.fixture, true);
  assert.equal(surface.name, 'autopilot');
  assert.equal(surface.command.ok, true);
  assert.equal(health.runtime.bridge, 'fixture');
});

test('maps live autoloop lane snapshots into existing lane board worker rows', () => {
  const state = {
    ...defaultLoopState(),
    running: true,
    mode: 'dry-run',
    pid: 4242
  };

  const worker = laneWorkerFromAutoloop(
    {
      lane: 'review',
      status: 'running',
      selected: '#421',
      action: 'reviewing',
      target: 'Human Review',
      maxConcurrent: 2,
      updatedAtMs: 1234
    },
    'review',
    state
  );

  assert.deepEqual(worker, {
    issue: '#421',
    title: '#421',
    action: 'reviewing',
    backend: 'Tauri dry-run',
    session: 'pid 4242',
    elapsed: 'Human Review',
    lane: 'review',
    status: 'running',
    waiting: true
  });
  assert.equal(laneWorkerFromAutoloop({ lane: 'main', status: 'idle' }, 'main', state), null);
  assert.equal(laneWorkerFromAutoloop({ lane: 'main', status: 'completed' }, 'main', state), null);
  assert.equal(laneWorkerFromAutoloop({ lane: 'main', status: 'skipped', selected: '#421' }, 'main', state), null);
  assert.equal(laneWorkerFromAutoloop({ lane: 'main', status: 'running', selected: 'none', action: 'tick_started' }, 'main', state), null);
});

test('autoloop stdout log omits repeated inactive skipped issue details', () => {
  const state = appendAutoloopLine(defaultLoopState(), {
    atMs: Date.now(),
    stream: 'stdout',
    line: '- I_kwDOSZP6c88AAAABB2ahNQ #13 reason=state is not active',
    event: {
      event: 'autopilot_cli_line',
      payload: {
        kind: '-',
        raw: '- I_kwDOSZP6c88AAAABB2ahNQ #13 reason=state is not active',
        fields: {}
      }
    }
  });

  assert.equal(state.recentLines.length, 0);
});

test('autoloop log omits no-op lane heartbeat events', () => {
  let state = defaultLoopState();
  state = appendAutoloopLine(state, {
    atMs: Date.now(),
    stream: 'stdout',
    line: 'autopilot_loop_lane lane=review status=running action=tick_started selected=none',
    event: {
      event: 'autopilot_loop_lane',
      payload: {
        lane: 'review',
        status: 'running',
        action: 'tick_started',
        selected_issue: null
      }
    }
  });
  state = appendAutoloopLine(state, {
    atMs: Date.now(),
    stream: 'stdout',
    line: 'autopilot_loop_lane lane=merge status=skipped action=lane_tick_skipped selected=none',
    event: {
      event: 'autopilot_loop_lane',
      payload: {
        lane: 'merge',
        status: 'skipped',
        action: 'lane_tick_skipped',
        selected_issue: null
      }
    }
  });

  assert.equal(state.recentLines.length, 0);
});

test('autoloop stdout log omits child lane idle stop lines', () => {
  const lines = [
    'merge_once=stopped reason=no_merging_issue',
    'merge_loop=stopped reason=no_merging_issue iterations=1 slot=1',
    'review_loop=stopped reason=no_agent_review_issue iterations=1'
  ];
  let state = defaultLoopState();

  for (const line of lines) {
    state = appendAutoloopLine(state, {
      atMs: Date.now(),
      stream: 'stdout',
      line,
      event: {
        event: 'autopilot_cli_line',
        payload: {
          kind: line.split('=')[0],
          raw: line,
          fields: {}
        }
      }
    });
  }

  assert.equal(state.recentLines.length, 0);
});

test('autoloop stdout log omits routine status and clean checkout lines', () => {
  const lines = [
    'SHEA SYMPHONY STATUS',
    'integration gaps:',
    '- GitHub Project v2 PR linking still uses an issue comment/autolink strategy rather than a first-class relationship.',
    '- GitHub Project v2 live write methods use `gh api graphql`; keep using `--write` for mutating CLI commands.',
    'canonical_checkout_refresh=ff_only upstream=origin/main head_before=old upstream_head=new head_after=new',
    'canonical_checkout root=/repo branch=main upstream=origin/main clean=true tracked_dirty=0 untracked=0 unclassified=0 migrated=0 quarantine=/repo/.tmp'
  ];
  let state = defaultLoopState();

  for (const line of lines) {
    state = appendAutoloopLine(state, {
      atMs: Date.now(),
      stream: 'stdout',
      line,
      event: {
        event: 'autopilot_cli_line',
        payload: {
          kind: line.split(/[ :=]/)[0],
          raw: line,
          fields: {}
        }
      }
    });
  }

  assert.equal(state.recentLines.length, 0);
});

test('lane throughput board keeps independent running and queued lane work visible', () => {
  const board = buildLaneThroughputBoard({
    queueIssues: [
      { id: '#410', title: 'Main active', lane: 'Main', state: 'In Progress', nextSkill: 'Manual Main', tone: 'success' },
      { id: '#411', title: 'Main queued one', lane: 'Main', state: 'Todo', nextSkill: 'Manual Main', tone: 'neutral' },
      { id: '#412', title: 'Main queued two', lane: 'Main', state: 'Todo', nextSkill: 'Manual Main', tone: 'neutral' },
      { id: '#421', title: 'Review active', lane: 'Review', state: 'Agent Review', nextSkill: 'Manual Review', tone: 'success' },
      { id: '#430', title: 'Merge queued', lane: 'Merge', state: 'Merging', nextSkill: 'Manual Merge', tone: 'success' }
    ],
    laneWorkers: {
      main: [{ issue: '#410', title: '#410', action: 'implementing', backend: 'codex', session: 'run/main', status: 'running', waiting: true }],
      review: [{ issue: '#421', title: '#421', action: 'reviewing', backend: 'gemini', session: 'run/review', status: 'running', waiting: true }],
      merge: []
    },
    laneSnapshots: {
      main: { lane: 'main', status: 'running', action: 'tick_started', maxConcurrent: 1 },
      review: { lane: 'review', status: 'running', action: 'tick_started', maxConcurrent: 1 },
      merge: { lane: 'merge', status: 'idle', maxConcurrent: 1 }
    },
    issueTitleById: new Map([
      ['#410', 'Main active'],
      ['#421', 'Review active']
    ])
  });

  const main = board.find((lane) => lane.laneKey === 'main');
  const review = board.find((lane) => lane.laneKey === 'review');
  const merge = board.find((lane) => lane.laneKey === 'merge');

  assert.equal(main.runningCount, 1);
  assert.equal(main.queuedCount, 2);
  assert.deepEqual(main.issues.map((issue) => issue.id), ['#410', '#411', '#412']);
  assert.match(main.statusText, /running 1 · queued 2/);
  assert.equal(review.runningCount, 1);
  assert.equal(review.queuedCount, 0);
  assert.equal(merge.runningCount, 0);
  assert.equal(merge.queuedCount, 1);
  assert.deepEqual(merge.issues.map((issue) => issue.id), ['#430']);
});

test('Codex transcript parser renders conversation turns, tool calls, outputs, final answers, and usage', () => {
  const transcript = [
    JSON.stringify({ type: 'message', item: { role: 'user', content: [{ text: 'Please inspect status.' }] } }),
    JSON.stringify({ type: 'function_call', item: { name: 'functions.exec_command', arguments: JSON.stringify({ cmd: 'git status --short' }), status: 'completed' } }),
    JSON.stringify({ type: 'function_call_output', item: { name: 'functions.exec_command', output: '## branch\n M app/src/file.ts' } }),
    JSON.stringify({ type: 'message', item: { role: 'assistant', content: 'I found one modified file.' } }),
    JSON.stringify({ type: 'final_answer', item: { role: 'assistant', content: 'Done.' } }),
    JSON.stringify({ type: 'response.completed', response: { usage: { input_tokens: 10, output_tokens: 5, total_tokens: 15 } } })
  ].join('\n');

  const parsed = parseCodexTranscriptJsonl(transcript);

  assert.equal(parsed.status, 'available');
  assert.equal(parsed.summary.userTurns, 1);
  assert.equal(parsed.summary.assistantTurns, 2);
  assert.equal(parsed.summary.toolCalls, 1);
  assert.equal(parsed.summary.tokenUsage, 'input 10 · output 5 · total 15');
  assert.equal(parsed.events.find((event) => event.kind === 'tool_call').title, 'functions.exec_command');
  assert.match(parsed.events.find((event) => event.kind === 'tool_call').body, /cmd: git status --short/);
  assert.match(parsed.events.find((event) => event.kind === 'tool_output').body, /modified file|## branch/);
});

test('Codex transcript parser marks malformed still-growing JSONL as partial', () => {
  const parsed = parseCodexTranscriptJsonl(`${JSON.stringify({ type: 'message', item: { role: 'user', content: 'hi' } })}\n{"type":`);

  assert.equal(parsed.status, 'partial');
  assert.equal(parsed.malformedLines, 1);
  assert.equal(parsed.events.some((event) => event.title === 'Malformed JSONL line'), true);
});

test('missing transcript state is local-only and explicit', () => {
  const unavailable = transcriptUnavailable('No local transcript candidate was found.');

  assert.equal(unavailable.status, 'unavailable');
  assert.equal(unavailable.localOnly, true);
  assert.match(unavailable.reason, /No local transcript/);
});

test('heartbeat classifier separates running, stale, stopped, and unavailable states', () => {
  const now = 10_000;

  assert.equal(classifyHeartbeat({ running: true, lanes: { main: { updatedAtMs: now - 15_000 } } }, 'main', now).state, 'running');
  assert.equal(classifyHeartbeat({ running: true, lanes: { main: { updatedAtMs: now - 180_000 } } }, 'main', now).state, 'stale');
  assert.equal(classifyHeartbeat({ running: false, lanes: { main: { updatedAtMs: now - 180_000 } } }, 'main', now).state, 'stopped');
  assert.equal(classifyHeartbeat(null, 'main', now).state, 'unavailable');
});

test('project read cooldown preserves last stable review queue visibility', () => {
  const view = buildViewModel({
    generatedAt: new Date().toISOString(),
    workflowPath: 'workflows/shea-symphony.md',
    commands: {
      githubQueue: {
        ok: false,
        skipped: true,
        projectReadPaused: true,
        rateLimitResetAtMs: Date.now() + 60_000,
        signal: 'project-rate-limit-cooldown',
        stderr: 'Project read paused after rate limit.'
      }
    },
    githubQueue: {
      source: 'project state · last stable during Project read cooldown',
      projectReadPaused: true,
      rateLimitResetAtMs: Date.now() + 60_000,
      totalOpen: 1,
      laneCounts: { main: 0, review: 1, merge: 0 },
      stateCounts: { 'Agent Review': 1 },
      issues: [
        {
          identifier: '#405',
          title: 'Make autopilot lanes independently throughput-oriented',
          state: 'Agent Review',
          url: 'https://github.com/Alive24/shea-symphony/issues/405'
        }
      ]
    },
    healthy: true
  });

  const board = buildLaneThroughputBoard({ queueIssues: view.queueIssues });
  const review = board.find((lane) => lane.laneKey === 'review');

  assert.equal(view.dataSource.label, 'Project reads paused');
  assert.match(view.dataSource.detail, /GitHub Project read paused until/);
  assert.deepEqual(review.issues.map((issue) => [issue.id, issue.title]), [
    ['#405', 'Make autopilot lanes independently throughput-oriented']
  ]);
  assert.equal(review.queuedCount, 1);
});

test('default background refresh defers doctor surface', () => {
  assert.deepEqual(defaultBackgroundReadSurfaces, ['githubQueue', 'skills', 'sessions', 'status']);
  assert.equal(defaultBackgroundReadSurfaces.includes('doctor'), false);
  assert.equal(defaultBackgroundReadSurfaces.includes('autopilot'), false);
  assert.equal(defaultBackgroundReadSurfaces.includes('review'), false);
  assert.equal(projectCooldownReadSurfaces.includes('doctor'), true);
});

test('lane throughput board surfaces blocked idle and completed lane results', () => {
  const board = buildLaneThroughputBoard({
    laneSnapshots: {
      main: { lane: 'main', status: 'blocked', action: 'readiness_blocked', blockedCount: 1 },
      review: { lane: 'review', status: 'completed', action: 'lane_tick_completed', completedCount: 1 },
      merge: { lane: 'merge', status: 'idle', idleCount: 1 }
    }
  });

  const main = board.find((lane) => lane.laneKey === 'main');
  const review = board.find((lane) => lane.laneKey === 'review');
  const merge = board.find((lane) => lane.laneKey === 'merge');

  assert.equal(main.blockedCount, 1);
  assert.equal(main.tone, 'danger');
  assert.deepEqual(main.issues.map((issue) => [issue.kind, issue.title]), [['blocked', 'readiness_blocked']]);
  assert.equal(review.completedCount, 1);
  assert.deepEqual(review.issues.map((issue) => issue.kind), ['completed']);
  assert.equal(merge.idleCount, 1);
  assert.deepEqual(merge.issues.map((issue) => issue.kind), ['idle']);
});

test('view model uses CLI autopilot parked queues for human todo issues', () => {
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
        durationMs: 12,
        stderr: '',
        stdoutPreview: '{}'
      }
    },
    autopilot: {
      lanes: [],
      parked_queues: [
        {
          name: 'Need to Clarify',
          state: 'Need to Clarify',
          issues: [{ identifier: '#501', title: 'Clarify issue contract' }]
        },
        {
          name: 'Need Human Input',
          state: 'Need Human Input',
          issues: [{ identifier: '#502', title: 'Operator decision needed' }]
        },
        {
          name: 'Human Review',
          state: 'Human Review',
          issues: [{ identifier: '#503', title: 'Approve review evidence', assignees: ['Alive24'] }]
        }
      ]
    },
    healthy: true
  });

  assert.deepEqual(
    view.queueIssues
      .map((issue) => [issue.id, issue.state, issue.lane])
      .sort((left, right) => left[0].localeCompare(right[0])),
    [
      ['#501', 'Need to Clarify', 'Human'],
      ['#502', 'Need Human Input', 'Human'],
      ['#503', 'Human Review', 'Human']
    ]
  );
  assert.deepEqual(view.queueIssues.find((issue) => issue.id === '#503').assignees, ['Alive24']);
  assert.ok(view.attentionTasks.every((task) => task.sourceLabel === 'Autopilot plan'));
});

test('human review project state does not appear in the review lane board queue', () => {
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
        durationMs: 12,
        stderr: '',
        stdoutPreview: '{}'
      }
    },
    autopilot: {
      lanes: [
        {
          lane: 'review',
          status: 'idle',
          selected_issue: {
            identifier: '#364',
            title: 'Operator approval pending',
            state: 'Human Review'
          },
          action: 'idle',
          reason: 'human review is parked for the operator'
        }
      ],
      parked_queues: []
    },
    healthy: true
  });

  const issue = view.queueIssues.find((item) => item.id === '#364');
  assert.equal(issue.state, 'Human Review');
  assert.equal(issue.lane, 'Human');
  assert.equal(issue.nextSkill, 'Human Review');
  assert.equal(view.projectWorkerMatch.lanes.find((lane) => lane.lane === 'Review').project, 0);
});

test('fixture overview feeds first-screen human todo and lane board data', () => {
  const overview = buildFixtureOverview(true);
  const view = buildViewModel(overview);

  assert.equal(view.dataSource.mode, 'fixture');
  assert.ok(view.queueIssues.some((issue) => issue.id === '#421' && issue.state === 'Need Human Input'));
  assert.ok(view.queueIssues.some((issue) => issue.id === '#418' && issue.lane === 'Human'));
  assert.ok(view.queueIssues.some((issue) => issue.id === '#430' && issue.lane === 'Merge'));
  assert.equal(view.laneWorkers.main.length, 0);
  assert.equal(view.laneWorkers.review.length, 0);
  assert.equal(view.laneWorkers.merge.length, 0);
  assert.ok(view.readPathMap.some((path) => path.id === 'tauri-bridge'));
});

test('view model renders object-shaped selected issues as issue references', () => {
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
        durationMs: 12,
        stderr: '',
        stdoutPreview: '{}'
      }
    },
    autopilot: {
      lanes: [
        {
          lane: 'main',
          status: 'ready',
          selected_issue: { identifier: '#512', title: 'Object issue should not leak' },
          action: null,
          reason: { title: 'Object reason should become text' }
        }
      ],
      parked_queues: []
    },
    healthy: true
  });

  const queued = view.queueIssues.find((issue) => issue.id === '#512');
  assert.equal(queued.title, 'Object issue should not leak');
  assert.match(queued.evidence, /Object reason should become text/);
  assert.equal(view.laneWorkers.main.length, 0);
});

test('view model exposes unresumed in-progress runtime issues as main queue rows', () => {
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
        durationMs: 12,
        stderr: '',
        stdoutPreview: '{}'
      }
    },
    autopilot: {
      lanes: [],
      parked_queues: [],
      active_issues: [
        {
          lane: 'main',
          identifier: '#364',
          backend: 'codex',
          session_id: null
        }
      ]
    },
    healthy: true
  });

  const queued = view.queueIssues.find((issue) => issue.id === '#364');
  assert.equal(queued.state, 'In Progress');
  assert.equal(queued.lane, 'Main');
  assert.match(queued.recommended, /no worker session is visible/);
  assert.equal(view.laneWorkers.main.length, 0);
});

test('lane board queue excludes backlog project items', () => {
  const view = buildViewModel({
    generatedAt: new Date().toISOString(),
    workflowPath: 'workflows/shea-symphony.md',
    commands: {
      githubQueue: {
        ok: true,
        args: ['autopilot', 'plan', 'workflows/shea-symphony.md', '--json'],
        exitCode: 0,
        signal: null,
        timedOut: false,
        durationMs: 12,
        stderr: '',
        stdoutPreview: '{}'
      }
    },
    githubQueue: {
      source: 'test queue',
      issues: [
        {
          identifier: '#329',
          title: 'Add a Dogfood session workflow and skill',
          state: 'Project Backlog'
        },
        {
          identifier: '#330',
          title: 'Dogfood: 2026-05-19 manual lane readiness run',
          state: 'Backlog'
        },
        {
          identifier: '#332',
          title: 'Already completed issue',
          state: 'Done'
        },
        {
          identifier: '#331',
          title: 'Executable main issue',
          state: 'Todo'
        }
      ]
    },
    healthy: true
  });

  assert.deepEqual(view.queueIssues.map((issue) => issue.id), ['#331']);
  assert.deepEqual(view.laneProjectIssues.main.map((issue) => issue.id), ['#331']);
});

test('read surface payloads incrementally replace pending overview commands', () => {
  const surface = buildFixtureReadSurface('autopilot', true);
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

  const merged = mergeReadSurface(overview, surface);
  const view = buildViewModel(merged);

  assert.equal(merged.commands.autopilot.ok, true);
  assert.equal(merged.autopilot.lanes.length, 3);
  assert.ok(view.laneSummaries.some((lane) => lane.sourceLabel === 'Live autopilot'));
});

test('offline fallback does not present fake Project or worker work', () => {
  const view = buildViewModel(null);

  assert.equal(view.dataSource.mode, 'offline');
  assert.equal(view.queueIssues.length, 0);
  assert.equal(view.projectWorkerMatch.projectTotal, 0);
  assert.equal(view.projectWorkerMatch.workerTotal, 0);
  assert.ok(view.attentionTasks.every((task) => task.type === 'Diagnostics'));
  assert.ok(view.laneSummaries.every((lane) => lane.active === 0));
});
