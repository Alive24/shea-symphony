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
import { defaultLoopState, laneWorkerFromAutoloop } from '../src/lib/tauriAutoloop.ts';

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
    title: 'reviewing',
    action: 'reviewing',
    backend: 'Tauri dry-run',
    session: 'pid 4242',
    elapsed: 'Human Review',
    lane: 'review'
  });
  assert.equal(laneWorkerFromAutoloop({ lane: 'main', status: 'idle' }, 'main', state), null);
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
          issues: [{ identifier: '#503', title: 'Approve review evidence' }]
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
      ['#501', 'Need to Clarify', 'Main'],
      ['#502', 'Need Human Input', 'Human'],
      ['#503', 'Human Review', 'Review']
    ]
  );
  assert.ok(view.attentionTasks.every((task) => task.sourceLabel === 'Autopilot plan'));
});

test('fixture overview feeds first-screen human todo and lane board data', () => {
  const overview = buildFixtureOverview(true);
  const view = buildViewModel(overview);

  assert.equal(view.dataSource.mode, 'fixture');
  assert.ok(view.queueIssues.some((issue) => issue.id === '#421' && issue.state === 'Need Human Input'));
  assert.equal(view.laneWorkers.main[0].issue, '#418');
  assert.equal(view.laneWorkers.review[0].issue, '#421');
  assert.equal(view.laneWorkers.merge[0].issue, '#430');
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

  assert.equal(view.laneWorkers.main[0].issue, '#512');
  assert.equal(view.laneWorkers.main[0].title, 'Object issue should not leak');
  assert.equal(view.laneWorkers.main[0].evidence, 'Object reason should become text');
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
