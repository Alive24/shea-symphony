import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { get } from 'svelte/store';

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
  operatorOverviewStore,
  projectCooldownReadSurfaces,
  requestOperatorLocalArtifactsRefresh
} from '../src/lib/operatorOverviewStore.ts';
import {
  LOCAL_ARTIFACT_READ_SURFACES,
  localArtifactRefreshEventDetail,
  shouldRequestLaneOverviewLocalRefresh
} from '../src/lib/localArtifactRefresh.ts';
import {
  buildLaneThroughputBoard
} from '../src/lib/viewModel/laneThroughput.ts';
import {
  completedProgressDisplay
} from '../src/lib/viewModel/completedWorktrees.ts';
import {
  appendAutoloopLine,
  defaultLoopState,
  laneWorkerFromAutoloop,
  laneWorkersFromAutoloopLines,
  operatorRunLogLines
} from '../src/lib/tauriAutoloop.ts';

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
  assert.deepEqual(laneWorkersFromAutoloopLines({
    ...state,
    startedAtMs: Date.now() - 100,
    recentLines: [{
      atMs: Date.now(),
      stream: 'stdout',
      line: 'Latest: review | #421 | waiting | review_agent',
      event: {
        event: 'autopilot_signal',
        payload: {
          visibility: 'operator',
          scope: 'lane',
          lane: 'review',
          issue: '#421',
          status: 'waiting',
          action: 'review_agent',
          message: '#421 review waiting review_agent'
        }
      }
    }]
  }, 'review').map((entry) => entry.issue), ['#421']);
});

test('autoloop lane workers clear issue after merge terminal events', () => {
  const now = Date.now();
  const state = {
    ...defaultLoopState(),
    running: true,
    startedAtMs: now - 100,
    recentLines: [
      {
        atMs: now,
        stream: 'stdout',
        line: 'Latest: merge | #428 | waiting | merge_decision',
        event: {
          event: 'autopilot_signal',
          payload: {
            visibility: 'operator',
            scope: 'lane',
            lane: 'merge',
            issue: '#428',
            status: 'waiting',
            action: 'merge_decision',
            message: '#428 merge waiting merge_decision'
          }
        }
      },
      {
        atMs: now + 1,
        stream: 'stdout',
        line: 'merging_pool_action=claim_field_terminal issue=#428 state=done result=merged outcome=applied',
        event: {
          event: 'autopilot_cli_line',
          payload: {
            kind: 'merging_pool_action',
            raw: 'merging_pool_action=claim_field_terminal issue=#428 state=done result=merged outcome=applied',
            fields: {
              issue: '#428',
              merging_pool_action: 'claim_field_terminal',
              state: 'done',
              result: 'merged',
              outcome: 'applied'
            }
          }
        }
      }
    ]
  };

  assert.deepEqual(laneWorkersFromAutoloopLines(state, 'merge'), []);
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

  assert.equal(state.recentLines.length, 1);
  assert.equal(operatorRunLogLines(state, state.recentLines, 'focus').length, 0);
  assert.equal(operatorRunLogLines(state, state.recentLines, 'normal').length, 0);
  assert.equal(operatorRunLogLines(state, state.recentLines, 'verbose').length, 1);
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
  state = appendAutoloopLine(state, {
    atMs: Date.now(),
    stream: 'stdout',
    line: 'autopilot_loop_lane lane=merge status=completed action=lane_tick_completed selected=none',
    event: {
      event: 'autopilot_loop_lane',
      payload: {
        lane: 'merge',
        status: 'completed',
        action: 'lane_tick_completed',
        selected_issue: null,
        work_unit_completed: false
      }
    }
  });

  assert.equal(state.recentLines.length, 3);
  assert.equal(operatorRunLogLines(state, state.recentLines, 'focus').length, 0);
  assert.equal(operatorRunLogLines(state, state.recentLines, 'normal').length, 0);
  assert.equal(operatorRunLogLines(state, state.recentLines, 'verbose').length, 3);
});

test('autoloop stdout log omits idle stop lines but keeps issue diagnostics in normal mode', () => {
  const lines = [
    'merge_once=stopped reason=no_merging_issue',
    'merge_loop=stopped reason=no_merging_issue iterations=1 slot=1',
    'review_loop=stopped reason=no_agent_review_issue iterations=1',
    'merge_loop_iteration=1 mode=write recover=true max_concurrent=3',
    'review_loop_iteration=1 mode=write max_concurrent=2',
    'polling: checking=false interval_ms=5000 next_poll_in_ms=5000',
    'activity: planned=0 running=0 retrying=0 skipped=0',
    'tokens: input=0 output=0 total=0 seconds_running=0',
    'event_log=/Users/example/.shea-symphony/logs/shea-symphony.jsonl',
    'Latest: merge | no-issue | idle | no_dispatchable_issue',
    'tracker_recovery action=already_applied mutation_type=set_state issue=#415 state=merging',
    'reason=pull request merge state is `DIRTY`',
    'pull_request=https://github.com/Alive24/shea-symphony/pull/427'
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

  assert.equal(operatorRunLogLines(state, state.recentLines, 'focus').length, 0);
  assert.deepEqual(
    operatorRunLogLines(state, state.recentLines, 'normal').map((entry) => entry.line),
    [
      'tracker_recovery action=already_applied mutation_type=set_state issue=#415 state=merging',
      'reason=pull request merge state is `DIRTY`',
      'pull_request=https://github.com/Alive24/shea-symphony/pull/427'
    ]
  );
  assert.equal(operatorRunLogLines(state, state.recentLines, 'verbose').length, lines.length);
});

test('autoloop log omits result summaries but keeps operator lane work and blockers', () => {
  let state = defaultLoopState();
  state = appendAutoloopLine(state, {
    atMs: Date.now(),
    stream: 'stdout',
    line: 'autopilot_loop_result',
    event: {
      event: 'autopilot_loop_result',
      payload: {
        work_units_completed_this_cycle: 0,
        completed_work_units: 83,
        lanes: [
          { lane: 'merge', status: 'completed', action: 'lane_tick_completed', selected_issue: null, work_unit_completed: false }
        ]
      }
    }
  });
  assert.equal(operatorRunLogLines(state, state.recentLines, 'normal').length, 0);

  state = appendAutoloopLine(state, {
    atMs: Date.now(),
    stream: 'stdout',
    line: 'autopilot_loop_lane',
    event: {
      event: 'autopilot_loop_lane',
      payload: {
        lane: 'main',
        status: 'completed',
        action: 'lane_tick_completed',
        selected_issue: { identifier: '#408' },
        work_unit_completed: true
      }
    }
  });
  state = appendAutoloopLine(state, {
    atMs: Date.now(),
    stream: 'stdout',
    line: 'autopilot_loop_lane',
    event: {
      event: 'autopilot_loop_lane',
      payload: {
        lane: 'merge',
        status: 'error',
        action: 'tick_failed',
        selected_issue: { identifier: '#408' },
        work_unit_completed: false
      }
    }
  });

  assert.equal(state.recentLines.length, 3);
  assert.equal(operatorRunLogLines(state, state.recentLines, 'normal').length, 2);
});

test('autoloop run logs only admit operator signals from legacy stdout', () => {
  let state = defaultLoopState();
  for (const [line, event] of [
    [
      'polling: checking=false interval_ms=5000 next_poll_in_ms=5000',
      { event: 'autopilot_signal', payload: { visibility: 'telemetry', scope: 'runtime', kind: 'polling', message: 'polling: checking=false interval_ms=5000 next_poll_in_ms=5000' } }
    ],
    [
      'Latest: main | no-issue | idle | no_dispatchable_issue | next=wait',
      { event: 'autopilot_signal', payload: { visibility: 'debug', scope: 'lane', lane: 'main', status: 'idle', action: 'no_dispatchable_issue', message: 'main idle no_dispatchable_issue' } }
    ],
    [
      'Latest: merge | #415 | waiting | merge_decision | Issue | next=repair',
      { event: 'autopilot_signal', payload: { visibility: 'operator', scope: 'lane', lane: 'merge', issue: '#415', status: 'waiting', action: 'merge_decision', message: '#415 merge waiting merge_decision' } }
    ]
  ]) {
    state = appendAutoloopLine(state, {
      atMs: Date.now(),
      stream: 'stdout',
      line,
      event
    });
  }

  assert.equal(state.recentLines.length, 3);
  const visible = operatorRunLogLines(state, state.recentLines, 'focus');
  assert.equal(visible.length, 1);
  assert.equal(visible[0].event.payload.issue, '#415');
});

test('autoloop running logs filter raw snapshot heartbeat lines', () => {
  const now = Date.now();
  const state = {
    ...defaultLoopState(),
    running: true,
    startedAtMs: now - 100,
    recentLines: [
      {
        atMs: now,
        stream: 'stdout',
        line: 'autopilot_loop_lane',
        event: {
          event: 'autopilot_loop_lane',
          payload: {
            lane: 'review',
            status: 'completed',
            action: 'lane_tick_completed',
            selected_issue: null,
            work_unit_completed: false
          }
        }
      },
      {
        atMs: now + 1,
        stream: 'stdout',
        line: 'autopilot_loop_result',
        event: {
          event: 'autopilot_loop_result',
          payload: {
            work_units_completed_this_cycle: 0,
            completed_work_units: 0,
            lanes: [
              { lane: 'review', status: 'completed', action: 'lane_tick_completed', selected_issue: null, work_unit_completed: false }
            ],
            settings: {
              main_max_concurrent: 3,
              review_max_concurrent: 2,
              merge_max_concurrent: 3
            }
          }
        }
      },
      {
        atMs: now + 2,
        stream: 'stdout',
        line: 'autopilot_loop_lane',
        event: {
          event: 'autopilot_loop_lane',
          payload: {
            lane: 'main',
            status: 'completed',
            action: 'lane_tick_completed',
            selected_issue: { identifier: '#408' },
            work_unit_completed: true
          }
        }
      }
    ]
  };

  const visible = operatorRunLogLines(state, state.recentLines, 'focus');

  assert.equal(visible.length, 1);
  assert.equal(visible[0].event.payload.selected_issue.identifier, '#408');
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

  assert.equal(operatorRunLogLines(state, state.recentLines, 'normal').length, 0);
  assert.equal(operatorRunLogLines(state, state.recentLines, 'verbose').length, lines.length);
});

test('autoloop run logs control supervisor and lifecycle visibility by verbosity', () => {
  const now = Date.now();
  let state = defaultLoopState();
  for (const event of [
    {
      event: 'autopilot_loop_supervisor',
      payload: { scheduler: 'independent', lanes: ['main', 'review', 'merge'] }
    },
    {
      event: 'autopilot_loop_status',
      payload: {
        phase: 'checking',
        message: 'checking Project, lane state, runtime state, and readiness',
        counts: { running: 0, blocked: 0 },
        blocked_reasons: []
      }
    },
    {
      event: 'autopilot_loop_status',
      payload: {
        phase: 'running',
        message: 'one or more lanes have useful work ready',
        counts: { running: 1, blocked: 0 },
        selected_issues: [{ identifier: '#415' }],
        blocked_reasons: []
      }
    }
  ]) {
    state = appendAutoloopLine(state, {
      atMs: now,
      stream: 'stdout',
      line: event.event,
      event
    });
  }

  assert.equal(operatorRunLogLines(state, state.recentLines, 'focus').length, 0);
  const normal = operatorRunLogLines(state, state.recentLines, 'normal');
  assert.equal(normal.length, 1);
  assert.equal(normal[0].event.payload.phase, 'running');
  assert.equal(operatorRunLogLines(state, state.recentLines, 'verbose').length, 3);

  state = appendAutoloopLine(state, {
    atMs: now + 1,
    stream: 'stdout',
    line: 'autopilot_loop_status',
    event: {
      event: 'autopilot_loop_status',
      payload: {
        phase: 'blocked',
        message: 'blocked state is visible and non-mutating',
        counts: { running: 0, blocked: 1 },
        blocked_reasons: ['main:preflight']
      }
    }
  });

  const focus = operatorRunLogLines(state, state.recentLines, 'focus');
  assert.equal(focus.length, 1);
  assert.equal(focus[0].event.payload.phase, 'blocked');
});

test('autoloop normal run logs keep issue diagnostics without routine heartbeats', () => {
  const now = Date.now();
  const state = {
    ...defaultLoopState(),
    running: true,
    startedAtMs: now - 100,
    recentLines: [
      {
        atMs: now,
        stream: 'stdout',
        line: 'polling: checking=false interval_ms=5000 next_poll_in_ms=5000',
        event: {
          event: 'autopilot_cli_line',
          payload: {
            kind: 'polling',
            raw: 'polling: checking=false interval_ms=5000 next_poll_in_ms=5000',
            fields: {}
          }
        }
      },
      {
        atMs: now + 1,
        stream: 'stdout',
        line: 'reason=Codex app-server stalled waiting for turn event issue=#428',
        event: {
          event: 'autopilot_cli_line',
          payload: {
            kind: 'reason',
            raw: 'reason=Codex app-server stalled waiting for turn event issue=#428',
            fields: { issue: '#428', reason: 'Codex app-server stalled waiting for turn event' }
          }
        }
      }
    ]
  };

  const normal = operatorRunLogLines(state, state.recentLines, 'normal');
  assert.equal(normal.length, 1);
  assert.equal(normal[0].event.payload.fields.issue, '#428');
  assert.equal(operatorRunLogLines(state, state.recentLines, 'verbose').length, 2);
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
  assert.equal(main.status, 'running');
  assert.equal(review.runningCount, 1);
  assert.equal(review.queuedCount, 0);
  assert.equal(merge.runningCount, 0);
  assert.equal(merge.queuedCount, 1);
  assert.equal(merge.status, 'queued');
  assert.deepEqual(merge.issues.map((issue) => issue.id), ['#430']);
});

test('lane throughput board suppresses default queued Todo metadata', () => {
  const board = buildLaneThroughputBoard({
    queueIssues: [
      {
        id: '#428',
        title: 'Fix Codex transcript rendering with timestamps and pagination',
        lane: 'Main',
        state: 'Todo',
        workerStatus: 'No worker visible',
        workerDetail: 'Project is waiting in this lane; no current worker session is visible.',
        evidence: 'project state · Todo',
        recommended: 'Run Issue Quality Gate before dispatch.',
        tone: 'neutral'
      }
    ]
  });

  const mainIssue = board.find((lane) => lane.laneKey === 'main').issues.find((issue) => issue.id === '#428');

  assert.equal(mainIssue.title, 'Fix Codex transcript rendering with timestamps and pagination');
  assert.equal(mainIssue.meta, '');
});

test('lane throughput board keeps concise queued exception metadata visible', () => {
  const board = buildLaneThroughputBoard({
    queueIssues: [
      {
        id: '#431',
        title: 'Repair review finding',
        lane: 'Main',
        state: 'Rework',
        workerStatus: 'No worker visible',
        workerDetail: 'Project is waiting in this lane; no current worker session is visible.',
        evidence: 'project state · Rework',
        recommended: 'Main lane can resume after checking rework evidence.',
        tone: 'warn'
      },
      {
        id: '#432',
        title: 'Investigate blocked dispatch',
        lane: 'Main',
        state: 'Blocked',
        workerStatus: 'Worker read unavailable',
        workerDetail: 'Worker session surface is unavailable; match status is unknown.',
        evidence: 'project state · Blocked · stale session',
        recommended: 'Inspect issue and diagnostics before choosing a lane.',
        tone: 'danger'
      },
      {
        id: '#433',
        title: 'Recover parked run',
        lane: 'Main',
        state: 'Todo',
        workerStatus: 'No worker visible',
        workerDetail: 'Recovered lane event needs operator attention.',
        evidence: 'project state · Todo · Recovered',
        recommended: 'Inspect recovered run evidence.',
        tone: 'warn'
      }
    ]
  });

  const main = board.find((lane) => lane.laneKey === 'main');
  const rework = main.issues.find((issue) => issue.id === '#431');
  const blocked = main.issues.find((issue) => issue.id === '#432');
  const recovered = main.issues.find((issue) => issue.id === '#433');

  assert.equal(rework.meta, 'Rework · Main lane can resume after checking rework evidence.');
  assert.equal(
    blocked.meta,
    'Blocked · Worker read unavailable · Worker session surface is unavailable; match status is unknown. · stale session · Inspect issue and diagnostics before choosing a lane.'
  );
  assert.equal(recovered.meta, 'Recovered lane event needs operator attention. · Recovered · Inspect recovered run evidence.');
});

test('lane throughput board keeps issue identity ahead of transient worker labels', () => {
  const board = buildLaneThroughputBoard({
    queueIssues: [
      { id: '#415', title: 'Separate lane board issue identity from transient worker status', lane: 'Main', state: 'In Progress', tone: 'success' },
      { id: '#416', title: 'Last-known issue title', lane: 'Review', state: 'Agent Review', workerStatus: 'No worker visible', tone: 'neutral' },
      { id: '#417', title: 'tick_started', lane: 'Merge', state: 'Merging', workerStatus: 'Waiting for agent response', tone: 'warn' }
    ],
    laneWorkers: {
      main: [
        {
          issue: '#415',
          title: 'Waiting for agent response',
          action: 'tick_started',
          backend: 'codex',
          session: 'run/main',
          status: 'running',
          waiting: true
        }
      ],
      review: [
        {
          issue: '#416',
          title: 'reviewing',
          action: 'reviewing',
          backend: 'gemini',
          session: 'run/review',
          status: 'running',
          waiting: true
        }
      ],
      merge: []
    },
    issueTitleById: new Map([
      ['#415', 'Project issue title wins'],
      ['#416', 'Last-known issue title']
    ])
  });

  const mainIssue = board.find((lane) => lane.laneKey === 'main').issues.find((issue) => issue.id === '#415');
  const reviewIssue = board.find((lane) => lane.laneKey === 'review').issues.find((issue) => issue.id === '#416');
  const mergeIssue = board.find((lane) => lane.laneKey === 'merge').issues.find((issue) => issue.id === '#417');

  assert.equal(mainIssue.title, 'Project issue title wins');
  assert.match(mainIssue.meta, /tick_started/);
  assert.equal(reviewIssue.title, 'Last-known issue title');
  assert.match(reviewIssue.meta, /reviewing/);
  assert.equal(mergeIssue.title, '#417');
  assert.match(mergeIssue.meta, /Waiting for agent response/);
});

test('lane board rendering omits handoff actions and manual skill labels', () => {
  const operatorDesk = readFileSync(new URL('../src/OperatorDesk.svelte', import.meta.url), 'utf8');
  const laneBoardSection = operatorDesk.slice(
    operatorDesk.indexOf('aria-label="Worker pickup and queue by lane"'),
    operatorDesk.indexOf('</section>', operatorDesk.indexOf('aria-label="Worker pickup and queue by lane"'))
  );
  const humanTodoSection = operatorDesk.slice(
    operatorDesk.indexOf('aria-label="Human operator issue queue"'),
    operatorDesk.indexOf('aria-label="Worker pickup and queue by lane"')
  );
  const laneDetail = readFileSync(new URL('../src/lib/LaneDetail.svelte', import.meta.url), 'utf8');

  assert.doesNotMatch(laneBoardSection, /handoff-actions|Copy Handoff Prompt|Next Skill|Manual Main|Manual Review|Manual Merge/);
  assert.doesNotMatch(laneDetail, /Next Skill|Manual Main|Manual Review|Manual Merge/);
  assert.match(humanTodoSection, /handoff-actions/);
  assert.match(humanTodoSection, /Open in/);
  assert.match(humanTodoSection, /Copy Handoff Prompt/);
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
  assert.equal(parsed.summary.rawRecords, 6);
  assert.equal(parsed.summary.unsupportedRecords, 0);
  assert.equal(parsed.events[0].timestamp, undefined);
  assert.equal(parsed.events.find((event) => event.kind === 'tool_call').title, 'functions.exec_command');
  assert.match(parsed.events.find((event) => event.kind === 'tool_call').body, /cmd: git status --short/);
  assert.match(parsed.events.find((event) => event.kind === 'tool_output').body, /modified file|## branch/);
});

test('Codex transcript parser renders rollout payload wrapper records with timestamps and unsupported counts', () => {
  const transcript = [
    JSON.stringify({
      timestamp: '2026-06-02T11:34:36.243Z',
      type: 'session_meta',
      payload: {
        id: '019e881d-344f-7cb2-bfbd-8c13b39f2a92',
        cwd: '/tmp/issue-410',
        cli_version: '0.0.0',
        source: 'codex',
        model_provider: 'openai'
      }
    }),
    JSON.stringify({
      timestamp: '2026-06-02T11:34:38.860Z',
      type: 'response_item',
      payload: { type: 'message', role: 'developer', content: [{ type: 'input_text', text: 'hidden instructions' }] }
    }),
    JSON.stringify({
      timestamp: '2026-06-02T11:34:38.863Z',
      type: 'event_msg',
      payload: { type: 'user_message', message: 'You are working on Shea Symphony issue #410.' }
    }),
    JSON.stringify({
      timestamp: '2026-06-02T11:34:49.527Z',
      type: 'event_msg',
      payload: { type: 'agent_message', phase: 'commentary', message: 'I am inspecting the transcript parser.' }
    }),
    JSON.stringify({
      timestamp: '2026-06-02T11:34:49.566Z',
      type: 'response_item',
      payload: { type: 'function_call', name: 'functions.exec_command', arguments: JSON.stringify({ cmd: 'rg transcript app/src' }), call_id: 'call_1' }
    }),
    JSON.stringify({
      timestamp: '2026-06-02T11:34:49.642Z',
      type: 'response_item',
      payload: { type: 'function_call_output', call_id: 'call_1', output: 'app/src/lib/viewModel/codexTranscript.ts' }
    }),
    JSON.stringify({
      timestamp: '2026-06-02T11:34:49.643Z',
      type: 'event_msg',
      payload: {
        type: 'token_count',
        info: {
          total_token_usage: { input_tokens: 100, output_tokens: 25, total_tokens: 125 }
        }
      }
    }),
    JSON.stringify({
      timestamp: '2026-06-02T11:34:50.000Z',
      type: 'turn_context',
      payload: { cwd: '/tmp/issue-410', model: 'gpt-5' }
    }),
    JSON.stringify({
      timestamp: '2026-06-02T11:42:09.799Z',
      type: 'event_msg',
      payload: { type: 'agent_message', phase: 'final_answer', message: 'Implemented locally.' }
    })
  ].join('\n');

  const parsed = parseCodexTranscriptJsonl(transcript);

  assert.equal(parsed.status, 'available');
  assert.equal(parsed.summary.rawRecords, 9);
  assert.equal(parsed.summary.readableEvents, 7);
  assert.equal(parsed.summary.unsupportedRecords, 2);
  assert.equal(parsed.summary.userTurns, 1);
  assert.equal(parsed.summary.assistantTurns, 2);
  assert.equal(parsed.summary.toolCalls, 1);
  assert.equal(parsed.summary.tokenUsage, 'input 100 · output 25 · total 125');
  assert.equal(parsed.events.find((event) => event.kind === 'assistant').timestamp, '2026-06-02T11:34:49.527Z');
  assert.equal(parsed.events.find((event) => event.kind === 'final').title, 'Final answer');
  assert.equal(parsed.events.some((event) => /hidden instructions/.test(event.body)), false);
});

test('Codex transcript parser renders app-server protocol JSONL without token delta noise', () => {
  const protocol = [
    JSON.stringify({
      direction: 'stdin',
      line: JSON.stringify({
        id: 3,
        method: 'turn/start',
        params: { input: [{ text: 'Please implement #414.' }] }
      })
    }),
    JSON.stringify({
      direction: 'stdout',
      line: JSON.stringify({
        method: 'item/agentMessage/delta',
        params: { itemId: 'msg_1', delta: 'tiny' }
      })
    }),
    JSON.stringify({
      direction: 'stdout',
      line: JSON.stringify({
        method: 'item/completed',
        params: {
          item: {
            type: 'agentMessage',
            text: 'I found the transcript candidate.',
            phase: 'commentary'
          }
        }
      })
    }),
    JSON.stringify({
      direction: 'stdout',
      line: JSON.stringify({
        method: 'item/started',
        params: {
          item: {
            type: 'commandExecution',
            command: 'rg transcript app/src',
            cwd: '/tmp/worktree',
            status: 'inProgress'
          }
        }
      })
    }),
    JSON.stringify({
      direction: 'stdout',
      line: JSON.stringify({
        method: 'item/completed',
        params: {
          item: {
            type: 'commandExecution',
            command: 'rg transcript app/src',
            aggregatedOutput: 'app/src/lib/LaneViews.svelte:350',
            status: 'completed',
            exitCode: 0
          }
        }
      })
    })
  ].join('\n');

  const parsed = parseCodexTranscriptJsonl(protocol);

  assert.equal(parsed.status, 'available');
  assert.equal(parsed.summary.userTurns, 1);
  assert.equal(parsed.summary.assistantTurns, 1);
  assert.equal(parsed.summary.toolCalls, 1);
  assert.equal(parsed.events.some((event) => event.body === 'tiny'), false);
  assert.equal(parsed.events.find((event) => event.kind === 'tool_call').title, 'rg transcript app/src');
  assert.match(parsed.events.find((event) => event.kind === 'tool_output').body, /LaneViews/);
});

test('Codex transcript parser marks malformed still-growing JSONL as partial', () => {
  const parsed = parseCodexTranscriptJsonl(`${JSON.stringify({ type: 'message', item: { role: 'user', content: 'hi' } })}\n{"type":`);

  assert.equal(parsed.status, 'partial');
  assert.equal(parsed.malformedLines, 1);
  assert.equal(parsed.events.some((event) => event.title === 'Malformed JSONL line'), true);
});

test('Codex conversation surface uses a deep link summary instead of transcript rendering', () => {
  const laneViews = readFileSync(new URL('../src/lib/LaneViews.svelte', import.meta.url), 'utf8');

  assert.match(laneViews, /Open in Codex/);
  assert.match(laneViews, /transcriptDeepLink/);
  assert.match(laneViews, /lastUserMessageAt/);
  assert.match(laneViews, /lastAssistantMessageAt/);
  assert.doesNotMatch(laneViews, /JsonLogView/);
  assert.doesNotMatch(laneViews, /transcriptPageEvents/);
});

test('missing transcript state is local-only and explicit', () => {
  const unavailable = transcriptUnavailable('No local transcript candidate was found.');

  assert.equal(unavailable.status, 'unavailable');
  assert.equal(unavailable.localOnly, true);
  assert.match(unavailable.reason, /No local transcript/);
});

test('heartbeat classifier separates running, stale, stopped, and unavailable states', () => {
  const now = 1_700_000_600_000;

  assert.equal(classifyHeartbeat({ running: true, lanes: { main: { updatedAtMs: now - 15_000 } } }, 'main', null, now).state, 'running');
  assert.equal(classifyHeartbeat({ running: true, lanes: { main: { updatedAtMs: now - 180_000 } } }, 'main', null, now).state, 'stale');
  assert.equal(classifyHeartbeat({ running: false, lanes: { main: { updatedAtMs: now - 180_000 } } }, 'main', null, now).state, 'stopped');
  assert.equal(classifyHeartbeat(null, 'main', null, now).state, 'unavailable');
});

test('heartbeat classifier treats zero, invalid, and missing timestamps as unavailable', () => {
  const now = 1_700_000_600_000;

  for (const updatedAtMs of [0, -1, Number.NaN, 'not-a-date', null, undefined]) {
    const summary = classifyHeartbeat({ running: true, lanes: { main: { updatedAtMs } }, recentLines: [] }, 'main', '#429', now);
    assert.equal(summary.state, 'unavailable');
    assert.equal(summary.lastHeartbeatMs, null);
    assert.equal(summary.lastHeartbeatAge, 'unavailable');
  }
});

test('latest lane event includes volatile event timestamp metadata', () => {
  const now = 1_700_000_600_000;
  const summary = classifyHeartbeat({
    running: true,
    lanes: { main: { updatedAtMs: now - 10_000 } },
    recentLines: [
      {
        atMs: now - 45_000,
        stream: 'stdout',
        line: 'Latest: main | #429 | running | implementation',
        event: {
          event: 'autopilot_signal',
          payload: {
            visibility: 'operator',
            lane: 'main',
            issue: '#429',
            message: '#429 implementation in progress'
          }
        }
      }
    ]
  }, 'main', '#429', now);

  assert.equal(summary.latestLaneEvent, '#429 implementation in progress');
  assert.equal(summary.latestLaneEventAtMs, now - 45_000);
  assert.equal(summary.latestLaneEventAge, '45s ago');
  assert.equal(summary.latestLaneEventSource, 'recentLines.autopilot_signal');
});

test('latest lane event falls back to persisted local lifecycle evidence when memory is empty', () => {
  const now = 1_700_000_600_000;
  const persistedAt = now - 15 * 60_000;
  const summary = classifyHeartbeat({
    running: true,
    lanes: { main: { updatedAtMs: 0 } },
    recentLines: []
  }, 'main', '#429', now, [
    { issue: '#428', lane: 'main', label: 'Different issue', time: new Date(now - 1000).toISOString() },
    { issue: '#429', lane: 'main', label: 'Main lane recorded handoff', detail: 'local event log', time: new Date(persistedAt).toISOString() }
  ]);

  assert.equal(summary.state, 'unavailable');
  assert.equal(summary.latestLaneEvent, 'Main lane recorded handoff · local event log');
  assert.equal(summary.latestLaneEventAtMs, persistedAt);
  assert.equal(summary.latestLaneEventAge, '15m ago');
  assert.equal(summary.latestLaneEventSource, 'localStatus.issueLifecycle');
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

test('lane overview route local refresh is bounded to the overview route', () => {
  assert.equal(shouldRequestLaneOverviewLocalRefresh('/lanes', 20_000, 0, 15_000), true);
  assert.equal(shouldRequestLaneOverviewLocalRefresh('/lanes', 24_000, 20_000, 15_000), false);
  assert.equal(shouldRequestLaneOverviewLocalRefresh('/lanes/408', 40_000, 0, 15_000), false);
  assert.equal(shouldRequestLaneOverviewLocalRefresh('/doctor', 40_000, 0, 15_000), false);
});

test('button-triggered local artifact refresh emits a local-only request', () => {
  assert.deepEqual(localArtifactRefreshEventDetail('lane-overview-local'), {
    source: 'lane-overview-local',
    force: true,
    localOnly: true
  });
});

test('local artifact refresh surfaces do not include Project or GitHub reads', () => {
  assert.deepEqual(LOCAL_ARTIFACT_READ_SURFACES, ['sessions', 'status']);
  assert.equal(LOCAL_ARTIFACT_READ_SURFACES.some((surface) => projectCooldownReadSurfaces.includes(surface)), false);
  assert.equal(LOCAL_ARTIFACT_READ_SURFACES.includes('githubQueue'), false);
  assert.equal(LOCAL_ARTIFACT_READ_SURFACES.includes('autopilot'), false);
  assert.equal(LOCAL_ARTIFACT_READ_SURFACES.includes('doctor'), false);
  assert.equal(LOCAL_ARTIFACT_READ_SURFACES.includes('review'), false);
});

test('local artifact refresh records in-flight and last-refreshed status', async () => {
  requestOperatorLocalArtifactsRefresh('test-lane-overview-local', false);

  assert.equal(get(operatorOverviewStore).localArtifactsRefresh.running, true);
  assert.equal(get(operatorOverviewStore).localArtifactsRefresh.remaining, 2);

  await waitFor(() => !get(operatorOverviewStore).localArtifactsRefresh.running);

  const status = get(operatorOverviewStore).localArtifactsRefresh;
  assert.equal(status.source, 'test-lane-overview-local');
  assert.equal(status.error, '');
  assert.equal(status.remaining, 0);
  assert.match(status.lastRefreshedAt, /^\d{4}-\d{2}-\d{2}T/);
  assert.equal(get(operatorOverviewStore).liveError, '');
});

test('completed worktree progress display is unknown without durable progress evidence', () => {
  const display = completedProgressDisplay(
    {
      state: 'Done',
      updatedAt: '2026-06-03T09:00:00Z',
      projectUpdatedAt: '2026-06-03T09:00:00Z',
      worktree: {
        lastProgressAt: null,
        lastProgressSource: 'unavailable',
        lastModified: 1_780_000_000_000
      }
    },
    () => 'fresh-looking'
  );

  assert.equal(display.label, 'Unknown');
  assert.equal(display.known, false);
  assert.match(display.title, /No durable handoff progress evidence/);
});

test('completed worktree progress display uses session provenance when present', () => {
  const display = completedProgressDisplay(
    {
      state: 'Done',
      worktree: {
        lastProgressAt: 1_780_000_000_000,
        lastProgressSource: 'session_registry.updated_at_ms',
        lastModified: 1_780_000_050_000
      }
    },
    (value) => `age:${value}`
  );

  assert.equal(display.label, 'age:1780000000000');
  assert.equal(display.known, true);
  assert.match(display.title, /session_registry\.updated_at_ms/);
});

test('lane throughput board keeps blocked rows but summarizes idle and completed in the header', () => {
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
  assert.equal(review.status, 'complete');
  assert.deepEqual(review.issues.map((issue) => issue.kind), []);
  assert.equal(merge.idleCount, 1);
  assert.equal(merge.status, 'idle');
  assert.deepEqual(merge.issues.map((issue) => issue.kind), []);
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
  assert.equal(Object.hasOwn(issue, 'nextSkill'), false);
  assert.equal(view.projectWorkerMatch.lanes.find((lane) => lane.lane === 'Review').project, 0);
});

test('view model keeps lane queue rows observational without next skill metadata', () => {
  const view = buildViewModel({
    generatedAt: new Date().toISOString(),
    workflowPath: 'workflows/shea-symphony.md',
    commands: {
      githubQueue: {
        ok: true,
        args: ['project', 'state', 'workflows/shea-symphony.md', '--json'],
        exitCode: 0,
        signal: null,
        timedOut: false,
        durationMs: 12,
        stderr: '',
        stdoutPreview: '{}'
      }
    },
    githubQueue: {
      source: 'GitHub Project',
      issues: [
        { identifier: '#415', title: 'Stable Project title', state: 'In Progress' },
        { identifier: '#421', title: 'Review evidence', state: 'Agent Review' },
        { identifier: '#430', title: 'Merge approved PR', state: 'Merging' }
      ]
    },
    healthy: true
  });

  assert.equal(view.queueIssues.length, 3);
  assert.ok(view.queueIssues.every((issue) => !Object.hasOwn(issue, 'nextSkill')));
  assert.ok(view.laneProjectIssues.main.every((issue) => !Object.hasOwn(issue, 'nextSkill')));
  assert.ok(view.laneProjectIssues.review.every((issue) => !Object.hasOwn(issue, 'nextSkill')));
  assert.ok(view.laneProjectIssues.merge.every((issue) => !Object.hasOwn(issue, 'nextSkill')));
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

async function waitFor(predicate, timeoutMs = 1000) {
  const startedAt = Date.now();
  while (!predicate()) {
    if (Date.now() - startedAt > timeoutMs) throw new Error('Timed out waiting for condition.');
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
}
