import { writable } from 'svelte/store';
import {
  fullEvents as fallbackEvents,
  laneSummaries as fallbackLaneSummaries
} from './data.ts';

type LooseRecord = Record<string, any>;

const DATA_MODE_KEY = 'shea-data-mode';
const FIXTURE_OVERVIEW_KEY = 'shea-fixture-overview';
const HANDOFF_TARGET_KEY = 'shea-handoff-target';
export const DATA_MODE_CHANGE_EVENT = 'shea-data-mode-change';
export const HANDOFF_TARGET_CHANGE_EVENT = 'shea-handoff-target-change';
export const HANDOFF_TARGETS = [
  { id: 'codex-app', label: 'Codex App' },
  { id: 'codex-cli', label: 'Codex CLI' },
  { id: 'github', label: 'GitHub Issue' }
];
export const defaultHandoffTargetStore = writable('codex-app');

export async function loadOverview(force = false, scope = 'full') {
  if (getDataMode() === 'fixture') return loadFixtureOverview(force);

  const params = new URLSearchParams();
  if (force) params.set('force', '1');
  if (scope !== 'full') params.set('scope', scope);
  const query = params.toString();
  const response = await fetch(`/api/overview${query ? `?${query}` : ''}`, {
    headers: { accept: 'application/json' }
  });
  if (!response.ok) {
    throw new Error(`overview request failed: ${response.status}`);
  }
  return response.json();
}

export function getDataMode() {
  if (typeof localStorage === 'undefined') return 'live';
  return localStorage.getItem(DATA_MODE_KEY) === 'fixture' ? 'fixture' : 'live';
}

export function setDataMode(mode: 'live' | 'fixture') {
  if (typeof localStorage === 'undefined') return;
  localStorage.setItem(DATA_MODE_KEY, mode);
  window.dispatchEvent(new CustomEvent(DATA_MODE_CHANGE_EVENT, { detail: { mode } }));
}

export function getDefaultHandoffTarget() {
  if (typeof localStorage === 'undefined') return 'codex-app';
  const saved = localStorage.getItem(HANDOFF_TARGET_KEY);
  return HANDOFF_TARGETS.some((target) => target.id === saved) ? saved : 'codex-app';
}

export function setDefaultHandoffTarget(targetId: string) {
  if (typeof localStorage === 'undefined') return;
  const nextTarget = HANDOFF_TARGETS.some((target) => target.id === targetId) ? targetId : 'codex-app';
  localStorage.setItem(HANDOFF_TARGET_KEY, nextTarget);
  defaultHandoffTargetStore.set(nextTarget);
  window.dispatchEvent(new CustomEvent(HANDOFF_TARGET_CHANGE_EVENT, { detail: { target: nextTarget } }));
}

export function resetFixtureOverview() {
  if (typeof localStorage === 'undefined') return;
  localStorage.removeItem(FIXTURE_OVERVIEW_KEY);
  window.dispatchEvent(new CustomEvent(DATA_MODE_CHANGE_EVENT, { detail: { mode: getDataMode(), reset: true } }));
}

export async function loadHealth() {
  const response = await fetch('/api/health', {
    headers: { accept: 'application/json' }
  });
  if (!response.ok) {
    throw new Error(`health request failed: ${response.status}`);
  }
  return response.json();
}

export async function loadReadSurface(name, force = false) {
  if (getDataMode() === 'fixture') {
    const fixture = loadFixtureOverview(force);
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

  const params = new URLSearchParams({ name });
  if (force) params.set('force', '1');
  const response = await fetch(`/api/read-surface?${params.toString()}`, {
    headers: { accept: 'application/json' }
  });
  if (!response.ok) {
    throw new Error(`read surface request failed: ${response.status}`);
  }
  return response.json();
}

function loadFixtureOverview(force = false) {
  if (typeof localStorage !== 'undefined' && !force) {
    const saved = localStorage.getItem(FIXTURE_OVERVIEW_KEY);
    if (saved) {
      try {
        return { ...JSON.parse(saved), generatedAt: new Date().toISOString() };
      } catch (_) {
        localStorage.removeItem(FIXTURE_OVERVIEW_KEY);
      }
    }
  }

  const overview = baseFixtureOverview();
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem(FIXTURE_OVERVIEW_KEY, JSON.stringify(overview));
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
      lane: 'Main',
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
      lane: 'Review',
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
      lane: 'Review',
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
      githubQueue: fixtureCommand(['gh', 'issue', 'list', '--json', 'projectItems'])
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
        main: 1,
        review: 2,
        merge: 1
      },
      operatorIssues: issues.filter((issue) => ['Need to Clarify', 'Need Human Input', 'Human Review'].includes(issue.state)),
      issues,
      source: 'fixture GitHub queue'
    },
    healthy: true
  };
}

export async function runCommand(payload) {
  const response = await fetch('/api/command', {
    method: 'POST',
    headers: { 'content-type': 'application/json', accept: 'application/json' },
    body: JSON.stringify(payload)
  });
  const result = await response.json();
  return { ok: response.ok && result.ok !== false, ...result };
}

export function mergeReadSurface(overview: any, surface: any) {
  if (!overview || !surface?.name) return overview;
  const next = {
    ...overview,
    generatedAt: surface.generatedAt ?? overview.generatedAt,
    scope: 'incremental',
    commands: {
      ...(overview.commands ?? {}),
      [surface.name]: surface.command
    }
  };

  if (surface.name === 'sessions') {
    next.sessionsText = surface.text ?? '';
  } else if (surface.name === 'local') {
    next.localStatus = surface.parsed ?? null;
  } else {
    next[surface.name] = surface.parsed ?? null;
  }

  next.healthy = Object.values(next.commands).some((result: any) => result?.ok);
  return next;
}

export function buildViewModel(overview: any): any {
  if (!overview) return fallbackViewModel('Waiting for local API.');

  const autopilot = overview.autopilot;
  const doctor = overview.doctor;
  const commands: LooseRecord = overview.commands ?? {};
  const githubQueue = overview.githubQueue;
  const generatedAt = overview.generatedAt ? new Date(overview.generatedAt) : null;
  const sessionReadState = parseSessionReadState(overview.sessionsText);
  const sessionWorkers = parseSessionWorkers(overview.sessionsText);
  const commandFailures = Object.entries(commands)
    .filter(([, result]: [string, any]) => result && !result.ok && !result.pending)
    .map(([name, result]) => ({
      id: name,
      title: `${labelForCommand(name)} unavailable`,
      type: 'Diagnostics',
      reason: commandDetail(result),
      action: 'Inspect',
      recommended: `Open diagnostics for ${labelForCommand(name)} and repair the underlying local dependency.`,
      evidence: commandEvidence(result),
      urgency: 'Needs repair',
      tone: 'warn',
      decisions: [
        {
          label: 'Retry',
          result: `Rerun ${labelForCommand(name)} from the web UI.`,
          writes: 'Read-only command.',
          commandAction: commandActionForDiagnostic(name)
        }
      ]
    }));

  const parkedTasks = buildParkedTasks(autopilot, githubQueue, commands.githubQueue);

  const readinessItems = [
    readinessFromCommand('Autopilot plan', commands.autopilot),
    readinessFromCommand('Doctor', commands.doctor),
    readinessFromCommand('Review status', commands.review),
    readinessFromCommand('Skills', commands.skills)
  ];

  const laneSummaries = ['main', 'review', 'merge'].map((lane) => {
    const lanePlan = (autopilot?.lanes ?? []).find((item) => item.lane === lane);
    const githubLaneCount = Number(githubQueue?.laneCounts?.[lane] ?? 0);
    const fallback = fallbackLaneSummaries.find((item) => item.name.toLowerCase() === lane);
    const source = laneSourceFor(lanePlan, commands.autopilot, overview, commands.githubQueue, githubQueue);
    return {
      name: titleCase(lane),
      href: `/lanes/${lane}`,
      active: lanePlan
        ? lanePlan.selected_issue
          ? 1
          : 0
        : githubQueue?.laneCounts
          ? githubLaneCount
        : source.countsReliable === false || source.provenance === 'live'
          ? 0
          : fallback?.active ?? 0,
      retrying: countLaneStatus(autopilot, lane, 'retrying'),
      blocked: lanePlan?.status === 'blocked' ? 1 : 0,
      latest: lanePlan
        ? `${lanePlan.action ?? 'No action'}: ${lanePlan.reason ?? lanePlan.status}`
        : githubQueue?.laneCounts
          ? `${githubLaneCount} open Project item${githubLaneCount === 1 ? '' : 's'} in lane states.`
        : source.provenance === 'live'
          ? 'Live autopilot read returned no selected issue for this lane.'
          : fallback?.latest ?? 'No live lane data.',
      posture: lanePlan?.status ?? (githubLaneCount > 0 ? 'queued' : source.provenance === 'live' ? 'idle' : fallback?.posture ?? 'unknown'),
      ...source
    };
  });

  const baseQueueIssues = buildQueueIssues(githubQueue, parkedTasks);
  const laneWorkers = {
    main: [...workersForLane(autopilot, 'main'), ...sessionWorkers.filter((worker) => worker.lane === 'main')],
    review: [...workersForLane(autopilot, 'review'), ...sessionWorkers.filter((worker) => worker.lane === 'review')],
    merge: [...workersForLane(autopilot, 'merge'), ...sessionWorkers.filter((worker) => worker.lane === 'merge')]
  };
  const queueIssues = annotateQueueIssuesWithWorkers(baseQueueIssues, laneWorkers, sessionReadState);
  const laneProjectIssues = {
    main: queueIssues.filter((issue) => issue.lane === 'Main'),
    review: queueIssues.filter((issue) => issue.lane === 'Review'),
    merge: queueIssues.filter((issue) => issue.lane === 'Merge')
  };
  const projectWorkerMatch = buildProjectWorkerMatch(laneProjectIssues, laneWorkers, sessionReadState);
  const workerMonitor = buildWorkerMonitor(sessionWorkers, autopilot, laneProjectIssues, sessionReadState);
  const currentFocus = buildCurrentFocus(queueIssues, projectWorkerMatch);

  const recentEvents = [
    ...eventRowsFromAutopilot(autopilot),
    ...Object.entries(commands).map(([name, result]: [string, any]) => ({
      time: generatedAt ? timeLabel(generatedAt) : 'now',
      lane: 'System',
      title: `${labelForCommand(name)} ${result.ok ? 'passed' : result.pending ? 'pending' : 'failed'}`,
      detail: result.ok
        ? `Completed in ${Math.round(result.durationMs / 1000)}s.`
        : firstLine(result.stderr || result.stdoutPreview || 'Command failed.')
    }))
  ].slice(0, 8);
  const fullEvents = recentEvents.length ? recentEvents : fallbackEvents;
  const stateDistribution = buildStateDistribution(
    autopilot,
    laneSummaries,
    parkedTasks,
    githubQueue,
    commands.githubQueue,
    queueIssues
  );
  const evidenceColumns = buildEvidenceColumns(fullEvents);
  const trackerSignals = buildTrackerSignals(overview, commands);
  const gateChecklist = buildGateChecklist();
  const timelineModel = buildTimelineModel();
  const capabilityMap = buildCapabilityMap(commands);
  const dataSource = buildDataSource(overview, commands, generatedAt);
  const issueIndex = buildIssueIndex(parkedTasks, laneWorkers, fullEvents, queueIssues);
  const commandHealth = buildCommandHealth(commands);
  const readPathMap = buildReadPathMap(commandHealth);
  const liveSignals = buildLiveSignals(overview, commands);
  const operatorBrief = buildOperatorBrief({
    attentionTasks: [...parkedTasks, ...commandFailures],
    laneSummaries,
    evidenceColumns,
    dataSource
  });

  return {
    attentionTasks: [...parkedTasks, ...commandFailures].slice(0, 6),
    laneSummaries,
    laneWorkers,
    laneProjectIssues,
    projectWorkerMatch,
    workerMonitor,
    currentFocus,
    sessionReadState,
    sessionWorkers,
    readinessItems,
    recentEvents: recentEvents.length ? recentEvents : fallbackEvents.slice(0, 3),
    fullEvents,
    stateDistribution,
    evidenceColumns,
    trackerSignals,
    gateChecklist,
    timelineModel,
    capabilityMap,
    dataSource,
    queueIssues,
    issueIndex,
    commandHealth,
    readPathMap,
    liveSignals,
    operatorBrief,
    generatedAtLabel: generatedAt ? timeLabel(generatedAt) : 'not checked',
    healthy: overview.healthy,
    fixture: overview.fixture === true,
    workflowPath: overview.workflowPath,
    raw: overview,
    doctor
  };
}

function fallbackViewModel(reason) {
  const offlineTask = {
    id: 'local-api',
    title: 'Live data unavailable',
    type: 'Diagnostics',
    reason,
    action: 'Start local server',
    recommended: 'Start or reconnect the local Shea web server before trusting Project or worker status.',
    evidence: reason,
    urgency: 'Offline',
    tone: 'danger'
  };
  const offlineLaneSummaries = ['Main', 'Review', 'Merge'].map((name) => ({
    name,
    href: `/lanes/${name.toLowerCase()}`,
    active: 0,
    retrying: 0,
    blocked: 0,
    latest: 'Live API unavailable; no Project or worker count is trusted.',
    posture: 'unknown',
    provenance: 'fallback',
    sourceLabel: 'Live API unavailable',
    sourceTone: 'danger'
  }));
  const offlineEvents = [
    {
      time: 'now',
      lane: 'System',
      title: 'Live data unavailable',
      detail: reason
    }
  ];
  const offlineDataSource = {
    mode: 'offline',
    label: 'Live data unavailable',
    tone: 'danger',
    freshness: 'No live API response',
    trust: 'Do not trust Project or worker counts yet',
    detail: reason
  };

  return {
    attentionTasks: [offlineTask],
    laneSummaries: offlineLaneSummaries,
    laneWorkers: null,
    laneProjectIssues: { main: [], review: [], merge: [] },
    projectWorkerMatch: buildProjectWorkerMatch({ main: [], review: [], merge: [] }, { main: [], review: [], merge: [] }, { status: 'unknown' }),
    workerMonitor: buildWorkerMonitor([], null, { main: [], review: [], merge: [] }, { status: 'unknown' }),
    currentFocus: null,
    sessionReadState: { status: 'unknown', detail: 'Session surface unavailable while local API is offline.' },
    sessionWorkers: [],
    readinessItems: [
      { label: 'Local API', status: 'Offline', tone: 'danger' },
      { label: 'Project queue', status: 'Not checked', tone: 'warn' },
      { label: 'Worker sessions', status: 'Not checked', tone: 'warn' },
      { label: 'Diagnostics', status: 'Not checked', tone: 'warn' }
    ],
    recentEvents: offlineEvents,
    fullEvents: offlineEvents,
    stateDistribution: buildStateDistribution(null, offlineLaneSummaries, []),
    evidenceColumns: buildEvidenceColumns(offlineEvents),
    trackerSignals: buildFallbackTrackerSignals(reason),
    gateChecklist: buildGateChecklist(),
    timelineModel: buildTimelineModel(),
    capabilityMap: buildCapabilityMap({}),
    dataSource: offlineDataSource,
    queueIssues: [],
    issueIndex: buildIssueIndex([], {
      main: [],
      review: [],
      merge: []
    }, offlineEvents, []),
    commandHealth: buildCommandHealth({}),
    readPathMap: buildReadPathMap(buildCommandHealth({})),
    operatorBrief: buildOperatorBrief({
      attentionTasks: [offlineTask],
      laneSummaries: offlineLaneSummaries,
      evidenceColumns: buildEvidenceColumns(offlineEvents),
      dataSource: offlineDataSource
    }),
    generatedAtLabel: 'offline',
    healthy: false,
    fixture: false,
    workflowPath: 'workflows/shea-symphony.md',
    raw: null
  };
}

function readinessFromCommand(label, result) {
  if (!result) return { label, status: 'Unknown', tone: 'warn' };
  if (result.ok) return { label, status: 'Ready', tone: 'success' };
  if (result.pending) return { label, status: 'Loading', tone: 'warn' };
  return { label, status: 'Blocked', tone: 'danger' };
}

function buildParkedTasks(autopilot, githubQueue, githubQueueResult) {
  const autopilotTasks = (autopilot?.parked_queues ?? []).flatMap((queue) =>
    (queue.issues ?? []).map((issue) => parkedTaskFromIssue({
      id: issue.identifier ?? issue.issue ?? queue.state ?? 'Issue',
      title: issue.title ?? `${queue.state ?? 'Parked'} queue item`,
      state: queue.state ?? queue.queue ?? 'Parked',
      reason: issue.reason ?? queue.reason ?? 'Issue is parked outside active lane dispatch.',
      recommended: queue.next_action ?? issue.next_action ?? 'Inspect the issue readback before routing.',
      evidence: issue.evidence ?? queue.evidence ?? 'Autopilot plan surfaced this item.',
      source: 'Autopilot plan'
    }))
  );
  if (autopilotTasks.length) return autopilotTasks;

  if (githubQueue?.operatorIssues?.length) {
    return githubQueue.operatorIssues.map((issue) => parkedTaskFromIssue({
      id: issue.identifier,
      title: issue.title,
      state: issue.state,
      reason: `${issue.state} issue is visible in the GitHub Project queue.`,
      recommended: issue.state === 'Human Review' ? 'Review evidence in chat Skill before routing.' : 'Inspect diagnostics and issue readback before routing.',
      evidence: `${githubQueue.source ?? 'GitHub queue'} · updated ${issue.updatedAt ?? 'unknown'}`,
      source: 'GitHub queue scan'
    }));
  }

  if (githubQueueResult?.ok && githubQueue?.totalOpen != null) return [];
  return [];
}

function parkedTaskFromIssue({ id, title, state, reason, recommended, evidence, source }) {
  return {
    id,
    title,
    type: state,
    reason,
    action: 'Inspect Issue',
    recommended,
    evidence,
    urgency: state,
    tone: state === 'Need Human Input' ? 'danger' : 'warn',
    sourceLabel: source,
    decisions: [
      {
        label: 'Open readback',
        result: 'Read project issue JSON and linked PR evidence.',
        writes: 'Read-only command.',
        commandAction: 'project-issue'
      },
      {
        label: 'Quality gate',
        result: 'Run the issue quality gate in dry-run mode.',
        writes: 'Dry-run command.',
        commandAction: 'quality-gate'
      }
    ]
  };
}

function laneSourceFor(lanePlan, autopilotResult, overview, githubQueueResult, githubQueue) {
  if (overview?.fixture === true) {
    return {
        provenance: 'fixture',
        sourceLabel: lanePlan ? 'Fixture autopilot' : 'Fixture fallback',
        sourceTone: 'warn',
        countsReliable: true
    };
  }

  if (lanePlan) {
    return {
      provenance: 'live',
      sourceLabel: 'Live autopilot',
      sourceTone: 'success',
      countsReliable: true
    };
  }

  if (githubQueue?.laneCounts && githubQueueResult?.ok) {
    return {
      provenance: 'live',
      sourceLabel: 'Live GitHub queue',
      sourceTone: 'success',
      countsReliable: true
    };
  }

  if (autopilotResult?.ok) {
    return {
      provenance: 'live',
      sourceLabel: 'Live empty lane',
      sourceTone: 'success',
      countsReliable: true
    };
  }

  if (autopilotResult) {
    if (autopilotResult.pending) {
      return {
        provenance: 'partial',
        sourceLabel: 'Pending slow read',
        sourceTone: 'warn',
        countsReliable: false
      };
    }
    return {
      provenance: autopilotResult.timedOut ? 'partial' : 'fallback',
      sourceLabel: autopilotResult.timedOut ? 'Timed-out fallback' : 'Fallback posture',
      sourceTone: autopilotResult.timedOut ? 'warn' : 'danger',
      countsReliable: false
    };
  }

  return {
    provenance: 'fallback',
    sourceLabel: 'Layout fallback',
    sourceTone: 'danger',
    countsReliable: false
  };
}

function countLaneStatus(autopilot, lane, status) {
  return (autopilot?.lane_activity ?? []).filter((item) => item.lane === lane && item.status === status)
    .length;
}

function eventRowsFromAutopilot(autopilot) {
  return (autopilot?.lanes ?? []).map((lane) => ({
    time: 'live',
    lane: titleCase(lane.lane ?? 'lane'),
    title: `${titleCase(lane.lane ?? 'Lane')} ${lane.status ?? 'status'}`,
    detail: `${lane.action ?? 'No action'}: ${lane.reason ?? 'No reason supplied.'}`
  }));
}

function buildStateDistribution(autopilot, laneSummaries, attentionTasks, githubQueue = null, githubQueueResult = null, queueIssues = []) {
  const rows = new Map([
    ['Backlog', stateRow('Backlog', 'neutral')],
    ['Todo', stateRow('Todo', 'neutral')],
    ['In Progress', stateRow('In Progress', 'success')],
    ['Agent Review', stateRow('Agent Review', 'success')],
    ['Human Review', stateRow('Human Review', 'warn')],
    ['Merging', stateRow('Merging', 'success')],
    ['Need Human Input', stateRow('Need Human Input', 'danger')],
    ['Rework', stateRow('Rework', 'warn')]
  ]);

  if (githubQueueResult?.ok && githubQueue?.stateCounts) {
    for (const [state, count] of Object.entries(githubQueue.stateCounts)) {
      const normalized = normalizeStateName(state);
      if (['Backlog', 'Done', 'No Project'].includes(normalized)) continue;
      bump(rows, normalized, Number(count ?? 0), githubQueue.source ?? 'GitHub Project queue', 'live');
    }
  } else if (queueIssues?.length) {
    for (const issue of queueIssues) {
      bump(rows, normalizeStateName(issue.state), 1, issue.source ?? 'Project queue', 'live');
    }
  } else {
    for (const lane of laneSummaries) {
      const state = lane.name === 'Main' ? 'In Progress' : lane.name === 'Review' ? 'Agent Review' : 'Merging';
      bump(rows, state, Number(lane.active ?? 0), lane.sourceLabel, lane.provenance);
    }

    for (const task of attentionTasks) {
      bump(rows, normalizeStateName(task.type ?? task.urgency), 1, task.sourceLabel ?? 'Live attention', 'live');
    }
  }

  return [...rows.values()]
    .map((row) => {
      const provenance = row.provenance.has('live')
        ? 'live'
        : row.provenance.has('partial')
          ? 'partial'
          : row.provenance.has('fixture')
            ? 'fixture'
            : row.provenance.has('fallback')
              ? 'fallback'
              : 'empty';
      return {
        ...row,
        provenance,
        sourceLabel: row.sources.size ? [...row.sources].join(' + ') : 'No visible count'
      };
    })
    .filter((row) => row.count > 0 || ['In Progress', 'Agent Review', 'Merging'].includes(row.state));
}

function stateRow(state, tone) {
  return { state, count: 0, tone, sources: new Set(), provenance: new Set() };
}

function buildEvidenceColumns(events) {
  return ['System', 'Main', 'Review', 'Merge', 'Human Review']
    .map((lane) => ({
      lane,
      events: events.filter((event) => event.lane === lane).slice(0, 3)
    }))
    .filter((column) => column.events.length > 0);
}

function buildTrackerSignals(overview, commands) {
  const autopilot = overview?.autopilot;
  const doctor = overview?.doctor;
  const review = overview?.review;
  return [
    {
      label: 'ProjectV2 tracker',
      status: commands.autopilot?.ok ? 'Readable' : 'Unknown',
      tone: commands.autopilot?.ok ? 'success' : 'warn',
      detail: autopilot?.readiness?.reason ?? 'Autopilot plan is the primary tracker read for queue posture.'
    },
    {
      label: 'Status metadata',
      status: commands.doctor?.ok && !doctor?.blockers ? 'Ready' : commands.doctor?.ok ? 'Needs attention' : 'Unknown',
      tone: commands.doctor?.ok && !doctor?.blockers ? 'success' : 'warn',
      detail: doctor?.blockers
        ? `${doctor.blockers} doctor blocker${doctor.blockers === 1 ? '' : 's'} visible.`
        : 'Status option and workflow readiness are checked through doctor evidence.'
    },
    {
      label: 'Review evidence',
      status: commands.review?.ok ? 'Readable' : 'Unknown',
      tone: commands.review?.ok ? 'success' : 'warn',
      detail: Array.isArray(review?.recent)
        ? `${review.recent.length} recent review record${review.recent.length === 1 ? '' : 's'} surfaced.`
        : 'Review status feeds Agent Review and Human Review visibility.'
    },
    {
      label: 'Session surface',
      status: commands.sessions?.ok ? 'Readable' : 'Unknown',
      tone: commands.sessions?.ok ? 'success' : 'warn',
      detail: overview?.sessionsText || 'Session list is used to detect active foreground work.'
    }
  ];
}

function buildFallbackTrackerSignals(reason) {
  return [
    {
      label: 'Local API',
      status: 'Offline',
      tone: 'danger',
      detail: reason
    },
    {
      label: 'ProjectV2 tracker',
      status: 'Not checked',
      tone: 'warn',
      detail: 'Start the local server to read live ProjectV2 posture.'
    },
    {
      label: 'Review evidence',
      status: 'Fixture only',
      tone: 'warn',
      detail: 'Fallback data is useful for layout, not live routing.'
    }
  ];
}

function buildGateChecklist() {
  return [
    {
      label: 'Issue Quality Gate',
      status: 'Before dispatch',
      detail: 'Todo and Rework items must prove goal, scope, dependencies, guardrails, and verification.'
    },
    {
      label: 'Agent Review Gate',
      status: 'Before Human Review',
      detail: 'Main lane stops at Agent Review; independent review records pass evidence or findings.'
    },
    {
      label: 'Human Decision Gate',
      status: 'Before Merging',
      detail: 'Human Review needs explicit approval or confirmed rework routing evidence.'
    },
    {
      label: 'Merge Readback Gate',
      status: 'Before Done',
      detail: 'Merge lane verifies PR landing, Project state readback, and cleanup evidence.'
    }
  ];
}

function buildTimelineModel() {
  return [
    {
      lane: 'Main',
      writer: 'Persistent Workpad',
      evidence: 'Context, plan, work log, validation, PR handoff, and rework rounds.'
    },
    {
      lane: 'Review',
      writer: 'Append-only timeline',
      evidence: 'Queued/running/completed review state, finding classification, and supported checklist evidence.'
    },
    {
      lane: 'Human',
      writer: 'Decision note',
      evidence: 'Operator UAT result and literal approve-to-Merging or rework decision.'
    },
    {
      lane: 'Merge',
      writer: 'Append-only timeline',
      evidence: 'Mergeability, repair evidence, merge result, Project readback, and cleanup status.'
    }
  ];
}

function buildCapabilityMap(commands) {
  return [
    {
      label: 'Tracker client abstraction',
      state: commands.autopilot?.ok ? 'Observed' : 'Pending read',
      tone: commands.autopilot?.ok ? 'success' : 'warn'
    },
    {
      label: 'Status surface',
      state: commands.skills?.ok ? 'Observed' : 'Pending read',
      tone: commands.skills?.ok ? 'success' : 'warn'
    },
    {
      label: 'Independent review',
      state: commands.review?.ok ? 'Observed' : 'Pending read',
      tone: commands.review?.ok ? 'success' : 'warn'
    },
    {
      label: 'Doctor diagnostics',
      state: commands.doctor?.ok ? 'Observed' : 'Pending read',
      tone: commands.doctor?.ok ? 'success' : 'warn'
    },
    {
      label: 'Local checkout',
      state: commands.local?.ok ? 'Observed' : 'Pending read',
      tone: commands.local?.ok ? 'success' : 'warn'
    },
    {
      label: 'GitHub queue scan',
      state: commands.githubQueue?.ok ? 'Observed' : 'Pending read',
      tone: commands.githubQueue?.ok ? 'success' : 'warn'
    }
  ];
}

function buildCommandHealth(commands) {
  return ['githubQueue', 'autopilot', 'doctor', 'review', 'skills', 'sessions', 'local'].map((name) => {
    const result = commands[name];
    const status = result ? (result.ok ? 'Passed' : result.pending ? 'Pending' : 'Failed') : 'Not checked';
    const detail = result ? commandDetail(result) : 'No result captured from this overview command.';
    return {
      id: name,
      label: labelForCommand(name),
      status,
      tone: result ? (result.ok ? 'success' : result.pending ? 'warn' : 'danger') : 'warn',
      duration: result?.durationMs == null ? 'n/a' : `${Math.round(result.durationMs / 1000)}s`,
      detail,
      exit: result ? exitLabel(result) : 'n/a',
      impact: commandImpact(name, result),
      recommendation: commandRecommendation(name, result),
      args: result?.args?.join(' ') ?? 'not run'
    };
  });
}

function buildReadPathMap(commandHealth) {
  const byId = new Map((commandHealth ?? []).map((command) => [command.id, command]));
  return [
    {
      id: 'web-api',
      label: 'Web API',
      role: 'Loopback server and static UI',
      status: 'Available',
      tone: 'success',
      detail: 'The browser can read /api/health and render the cockpit.'
    },
    readPathNode(byId.get('autopilot'), {
      id: 'tracker',
      label: 'Tracker Posture',
      role: 'Autopilot plan',
      detail: 'Feeds lane counts, selected issues, and parked queues.'
    }),
    readPathNode(byId.get('githubQueue'), {
      id: 'github-queue',
      label: 'GitHub Queue',
      role: 'Open issue Project scan',
      detail: 'Feeds first-screen lane pulse and operator queue counts.'
    }),
    readPathNode(byId.get('doctor'), {
      id: 'readiness',
      label: 'Readiness',
      role: 'Doctor',
      detail: 'Feeds blockers, repair recommendations, and install health.'
    }),
    readPathNode(byId.get('review'), {
      id: 'review',
      label: 'Review Evidence',
      role: 'Review status',
      detail: 'Feeds Agent Review and Human Review freshness.'
    }),
    readPathNode(byId.get('skills'), {
      id: 'skills',
      label: 'Skill Coverage',
      role: 'Skills status',
      detail: 'Feeds installed Skill visibility and setup gaps.'
    }),
    readPathNode(byId.get('sessions'), {
      id: 'sessions',
      label: 'Foreground Sessions',
      role: 'Session list',
      detail: 'Feeds active agent session presence.'
    }),
    readPathNode(byId.get('local'), {
      id: 'local',
      label: 'Local Checkout',
      role: 'Git and build status',
      detail: 'Feeds branch, dirty count, worktree count, build, and binary readiness.'
    })
  ];
}

function readPathNode(command, fallback) {
  const timedOut = command?.exit?.startsWith('timeout');
  return {
    ...fallback,
    status: command?.status ?? 'Not checked',
    tone: command ? (timedOut || command.status === 'Pending' ? 'warn' : command.tone) : 'warn',
    detail: command ? command.impact : fallback.detail,
    signal: command ? command.detail : 'No read captured.',
    exit: command?.exit ?? 'n/a'
  };
}

function commandDetail(result) {
  if (result.ok) return firstLine(result.stdoutPreview || 'Command completed.');
  if (result.pending) return firstLine(result.stderr || 'Deferred to full overview.');
  if (result.timedOut) return `Timed out after ${Math.round((result.durationMs ?? 0) / 1000)}s.`;
  return firstLine(result.stderr || result.stdoutPreview || 'Command failed.');
}

function commandEvidence(result) {
  if (result.timedOut) {
    return `The command exceeded the Web overview timeout and was stopped with ${result.signal ?? 'a termination signal'}.`;
  }
  return result.stderr || result.stdoutPreview || 'No command output was captured.';
}

function exitLabel(result) {
  if (result.pending) return 'pending';
  if (result.timedOut) return `timeout / ${result.signal ?? 'terminated'}`;
  if (result.exitCode == null) return result.signal ?? 'n/a';
  return String(result.exitCode);
}

function commandImpact(name, result) {
  if (!result) return 'This read surface has not been checked yet.';
  if (result.pending) return 'This slower read surface is loading in the full overview pass.';
  if (result.ok) {
    const impacts = {
      autopilot: 'Lane queue posture and selected work can be trusted.',
      doctor: 'Readiness blockers and repair recommendations are visible.',
      review: 'Agent Review and Human Review evidence can be inspected.',
      skills: 'Installed Shea Skill coverage is observable.',
      sessions: 'Foreground agent session presence is observable.',
      local: 'Local checkout, build, binary, and worktree posture are observable.',
      githubQueue: 'Open issue Project status counts are available for the first-screen lane pulse.'
    };
    return impacts[name] ?? 'This read surface is available.';
  }
  const impacts = {
    autopilot: 'Lane counts may fall back to static posture and parked queues may be incomplete.',
    doctor: 'Readiness blockers may be hidden until Doctor returns.',
    review: 'Review freshness and Human Review evidence may be incomplete.',
    skills: 'Skill installation/readiness status may be hidden.',
    sessions: 'Active foreground sessions may be hidden.',
    local: 'Local checkout posture may be hidden.',
    githubQueue: 'First-screen lane pulse may be stale or rely on slower tracker reads.'
  };
  return impacts[name] ?? 'This read surface is degraded.';
}

function commandRecommendation(name, result) {
  if (!result) return 'Refresh overview after the local server is available.';
  if (result.ok) return 'Use this signal for observation.';
  if (result.pending) return 'Use fast overview for immediate posture; wait for full overview before trusting this surface.';
  if (result.timedOut) return 'Treat as slow read surface; inspect Diagnostics before trusting related counts.';
  return `Inspect ${labelForCommand(name)} output and local dependencies.`;
}

function buildDataSource(overview: any, commands: LooseRecord, generatedAt: Date | null) {
  if (overview?.fixture === true) {
    return {
      mode: 'fixture',
      label: 'Fixture data',
      tone: 'warn',
      freshness: generatedAt ? `Generated ${timeLabel(generatedAt)}` : 'Generated locally',
      trust: 'Safe for visual QA, not tracker routing',
      detail: 'GitHub reads and tracker writes are intentionally bypassed.'
    };
  }

  const commandResults = Object.values(commands) as any[];
  const passed = commandResults.filter((result) => result?.ok).length;
  const pending = commandResults.filter((result) => result?.pending).length;
  const total = commandResults.length;
  const allPassed = total > 0 && passed === total;
  return {
    mode: passed > 0 ? 'live' : 'degraded',
    label: allPassed ? 'Live tracker data' : pending > 0 ? 'Fast live data' : passed > 0 ? 'Partial live data' : 'No live data',
    tone: allPassed ? 'success' : passed > 0 ? 'warn' : 'danger',
    freshness: generatedAt ? `Checked ${timeLabel(generatedAt)}` : 'Not checked',
    trust:
      allPassed
        ? 'Usable for operator awareness'
        : pending > 0
          ? 'Fast readback only; full overview loading'
          : 'Confirm in chat Skills before routing',
    detail: `${passed}/${total || 0} overview commands passed${pending ? ` · ${pending} pending slow reads` : ''}.`
  };
}

function buildLiveSignals(overview, commands) {
  const skills = overview?.skills;
  const githubQueue = overview?.githubQueue;
  const skillSummary = skills?.summary ?? {};
  const sessionsText = overview?.sessionsText ?? '';
  const sessionReadState = parseSessionReadState(sessionsText);
  const activeSessionCount = parseSessionCount(sessionsText);
  const slowReads = ['autopilot', 'doctor', 'review'].map((id) => {
    const result = commands[id];
    return {
      id,
      label: labelForCommand(id),
      status: result?.ok ? 'live' : result?.pending ? 'loading' : result?.timedOut ? 'timeout' : result ? 'degraded' : 'unknown',
      tone: result?.ok ? 'success' : result?.pending || result?.timedOut ? 'warn' : 'danger'
    };
  });

  return [
    {
      id: 'sessions',
      label: 'Workers',
      value: sessionReadState.status === 'unavailable' ? '!' : activeSessionCount == null ? 'Read' : String(activeSessionCount),
      shortDetail:
        sessionReadState.status === 'unavailable'
          ? 'unavailable'
          : activeSessionCount === 0
            ? 'none active'
            : 'readable',
      detail:
        sessionReadState.status === 'unavailable'
          ? sessionReadState.detail
          : activeSessionCount === 0
            ? 'No foreground agent sessions'
            : sessionsText || 'Session list is readable',
      tone: commands.sessions?.ok && sessionReadState.status !== 'unavailable' ? 'success' : 'warn'
    },
    {
      id: 'skills',
      label: 'Skills',
      value: skillSummary.expected_skills == null ? 'Read' : `${skillSummary.expected_skills}`,
      shortDetail:
        skillSummary.blockers == null
          ? commands.skills?.ok
            ? 'readable'
            : 'pending'
          : `${skillSummary.blockers} blocker${skillSummary.blockers === 1 ? '' : 's'}`,
      detail:
        skillSummary.blockers == null
          ? commands.skills?.ok
            ? 'Skill surface readable'
            : 'Skill surface pending'
          : `${skillSummary.blockers} blocker${skillSummary.blockers === 1 ? '' : 's'} · ${skillSummary.codex_status ?? 'unknown'}`,
      tone: skillSummary.blockers > 0 ? 'warn' : commands.skills?.ok ? 'success' : 'warn'
    },
    {
      id: 'slow',
      label: 'Queue',
      value: githubQueue?.totalOpen == null ? 'Read' : String(githubQueue.totalOpen),
      shortDetail: githubQueue?.laneCounts
        ? `${githubQueue.laneCounts.main ?? 0}/${githubQueue.laneCounts.review ?? 0}/${githubQueue.laneCounts.merge ?? 0} lanes`
        : summarizeSlowReads(slowReads),
      detail: githubQueue?.stateCounts
        ? Object.entries(githubQueue.stateCounts).map(([state, count]) => `${state}: ${count}`).join(' · ')
        : slowReads.map((item) => `${item.label}: ${item.status}`).join(' · '),
      tone: commands.githubQueue?.ok
        ? 'success'
        : slowReads.some((item) => item.status === 'timeout' || item.status === 'degraded')
          ? 'warn'
          : 'success'
    },
    {
      id: 'local',
      label: 'Local',
      value: overview?.localStatus?.branch ?? 'Read',
      shortDetail: localStatusShortDetail(overview?.localStatus, commands.local),
      detail: localStatusDetail(overview?.localStatus, commands.local),
      tone: commands.local?.ok ? (overview?.localStatus?.dirtyCount > 0 ? 'warn' : 'success') : 'danger'
    }
  ];
}

function summarizeSlowReads(slowReads) {
  const timeouts = slowReads.filter((item) => item.status === 'timeout').length;
  if (timeouts) return `${timeouts} timeout${timeouts === 1 ? '' : 's'}`;
  const loading = slowReads.filter((item) => item.status === 'loading').length;
  if (loading) return `${loading} loading`;
  const live = slowReads.filter((item) => item.status === 'live').length;
  return `${live} live`;
}

function localStatusShortDetail(localStatus, command) {
  if (!command) return 'not checked';
  if (!command.ok) return 'unavailable';
  if (!localStatus) return 'readable';
  return `${localStatus.dirtyCount ?? 0} dirty`;
}

function localStatusDetail(localStatus, command) {
  if (!command) return 'Local repo status not checked';
  if (!command.ok) return 'Local repo status unavailable';
  if (!localStatus) return 'Local repo readable';
  const dirty = `${localStatus.dirtyCount ?? 0} dirty`;
  const worktrees = `${localStatus.worktreeCount ?? 0} worktrees`;
  const binary = localStatus.binaryPresent ? 'binary ready' : 'binary missing';
  return `${localStatus.head ?? 'unknown'} · ${dirty} · ${worktrees} · ${binary}`;
}

function parseSessionCount(text) {
  const value = String(text ?? '').trim();
  if (!value) return null;
  if (/agent_session_list=unavailable/.test(value)) return null;
  if (/agent_session_list=none/.test(value)) return 0;
  const countMatch = value.match(/(?:count|session_count)=(\d+)/);
  if (countMatch) return Number(countMatch[1]);
  const sessionLines = value.split('\n').filter((line) => line.trim() && !/agent_session_list=/.test(line));
  return sessionLines.length || null;
}

function parseSessionReadState(text) {
  const value = String(text ?? '').trim();
  if (!value) return { status: 'unknown', detail: 'Session list did not return text.' };
  if (/agent_session_list=unavailable/.test(value)) {
    const reason = value.match(/reason=([^\s]+)/)?.[1] ?? 'unknown';
    return { status: 'unavailable', detail: `Worker session surface unavailable: ${reason}.` };
  }
  if (/agent_session_list=none/.test(value)) {
    return { status: 'none', detail: 'No foreground agent sessions are visible.' };
  }
  return { status: 'readable', detail: 'Worker session surface is readable.' };
}

function parseSessionWorkers(text) {
  const value = String(text ?? '').trim();
  if (!value || /agent_session_list=none/.test(value)) return [];
  return value
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.startsWith('agent_session '))
    .map((line) => sessionWorkerFromFields(parseKeyValueLine(line)))
    .filter(Boolean);
}

function parseKeyValueLine(line) {
  const fields = {};
  const pattern = /(\w+)=("([^"]*)"|[^\s]+)/g;
  let match;
  while ((match = pattern.exec(line))) {
    fields[match[1]] = match[3] ?? match[2];
  }
  return fields;
}

function sessionWorkerFromFields(fields) {
  const session = fields.session ?? fields.session_name ?? fields.name;
  if (!session) return null;
  const lane = normalizeSessionLane(fields.lane ?? laneFromSessionName(session));
  if (!lane) return null;
  const issue = fields.issue ?? fields.issue_identifier ?? issueFromSessionName(session);
  const status = fields.status ?? (fields.attached === '1' ? 'attached' : 'running');
  return {
    issue: issue ?? session,
    title: fields.title ?? `${titleCase(lane)} worker session`,
    action: fields.action ?? 'Session registered in local worker surface.',
    backend: fields.backend ?? 'tmux/session',
    session,
    elapsed: status,
    evidence: fields.evidence ?? fields.attach_command ?? `session=${session}`,
    target: fields.target ?? status,
    lane,
    url: null
  };
}

function normalizeSessionLane(value) {
  const normalized = String(value ?? '').toLowerCase();
  if (normalized.includes('main')) return 'main';
  if (normalized.includes('review')) return 'review';
  if (normalized.includes('merge') || normalized.includes('merging')) return 'merge';
  return null;
}

function laneFromSessionName(session) {
  const value = String(session ?? '').toLowerCase();
  const match = value.match(/(?:^|-)(main|review|merge|merging)(?:-|$)/);
  return match?.[1];
}

function issueFromSessionName(session) {
  const value = String(session ?? '');
  const match = value.match(/(?:^|-)#?(\d+)(?:-|$)/);
  return match ? `#${match[1]}` : null;
}

function buildOperatorBrief({ attentionTasks, laneSummaries, evidenceColumns, dataSource }) {
  const sortedTasks = [...(attentionTasks ?? [])].sort(
    (left, right) => severityRank(right.tone) - severityRank(left.tone)
  );
  const focus = sortedTasks[0] ?? null;
  const skillCounts = new Map([
    ['Manual Main', { label: 'Manual Main', count: 0, tone: 'neutral' }],
    ['Manual Review', { label: 'Manual Review', count: 0, tone: 'neutral' }],
    ['Human Review', { label: 'Human Review', count: 0, tone: 'neutral' }],
    ['Doctor', { label: 'Doctor', count: 0, tone: 'neutral' }]
  ]);

  for (const task of attentionTasks ?? []) {
    const skill = skillForTask(task);
    const row = skillCounts.get(skill) ?? { label: skill, count: 0, tone: 'neutral' };
    row.count += 1;
    row.tone = toneForCount(row.count, task.tone);
    skillCounts.set(skill, row);
  }

  const lanes = (laneSummaries ?? []).map((lane) => ({
    name: lane.name,
    pressure: Number(lane.active ?? 0) + Number(lane.blocked ?? 0) + Number(lane.retrying ?? 0),
    tone:
      lane.sourceTone === 'danger'
        ? 'danger'
        : Number(lane.blocked ?? 0)
          ? 'danger'
          : lane.sourceTone === 'warn' || Number(lane.retrying ?? 0)
            ? 'warn'
            : 'success',
    provenance: lane.provenance ?? 'fallback',
    sourceLabel: lane.sourceLabel ?? 'Unknown source'
  }));
  const laneMax = Math.max(1, ...lanes.map((lane) => lane.pressure));
  const laneSources = new Set(lanes.map((lane) => lane.provenance));
  const sourceNote = laneSources.has('fallback')
    ? 'Lane counts include fallback posture'
    : laneSources.has('partial')
      ? 'Lane counts are partial'
      : laneSources.has('fixture')
        ? 'Fixture lane posture'
        : 'Lane counts from live reads';
  const evidence = (evidenceColumns ?? []).map((column) => ({
    lane: column.lane,
    count: column.events?.length ?? 0
  }));

  return {
    focus,
    trust: dataSource?.trust ?? 'Confirm in chat Skills before routing',
    sourceNote,
    skills: [...skillCounts.values()],
    lanes,
    laneMax,
    evidence
  };
}

function buildQueueIssues(githubQueue, attentionTasks = []) {
  const fromGithub = (githubQueue?.issues ?? [])
    .filter((issue) => issue.state && issue.state !== 'Backlog' && issue.state !== 'Done')
    .map((issue) => ({
      id: issue.identifier,
      title: issue.title,
      state: normalizeStateName(issue.state),
      lane: stateToLane(normalizeStateName(issue.state)),
      url: issue.url,
      updatedAt: issue.updatedAt,
      assignees: issue.assignees ?? [],
      labels: issue.labels ?? [],
      evidence: `${githubQueue.source ?? 'GitHub queue'} · ${issue.state}`,
      recommended: recommendationForQueueState(issue.state),
      tone: toneForState(normalizeStateName(issue.state)),
      source: 'githubQueue'
    }));

  if (fromGithub.length) return fromGithub.sort(queueIssueSort);

  return (attentionTasks ?? []).map((task) => queueIssueFromTask(task)).sort(queueIssueSort);
}

function annotateQueueIssuesWithWorkers(queueIssues: any[], laneWorkers: LooseRecord, sessionReadState: any = { status: 'unknown' }) {
  const workersByIssue = new Map();
  for (const workers of Object.values(laneWorkers ?? {}) as any[][]) {
    for (const worker of workers ?? []) {
      const id = normalizeIssueRef(worker.issue);
      if (!id) continue;
      if (!workersByIssue.has(id)) workersByIssue.set(id, []);
      workersByIssue.get(id).push(worker);
    }
  }

  return (queueIssues ?? []).map((issue) => {
    const workers = workersByIssue.get(normalizeIssueRef(issue.id)) ?? [];
    const workerCount = workers.length;
    const unavailable = sessionReadState.status === 'unavailable';
    return {
      ...issue,
      workerCount,
      workerStatus: unavailable ? 'Worker read unavailable' : workerCount ? 'Worker matched' : 'No worker visible',
      workerTone: workerCount ? 'success' : 'warn',
      workerDetail: unavailable
        ? 'Worker session surface is unavailable; match status is unknown.'
        : workerCount
        ? `${workerCount} worker${workerCount === 1 ? '' : 's'} visible for this Project item.`
        : 'Project is waiting in this lane; no current worker session is visible.',
      nextSkill: skillForQueueIssue(issue)
    };
  });
}

function skillForQueueIssue(issue) {
  const state = normalizeStateName(issue.state);
  if (state === 'Agent Review') return 'Manual Review';
  if (state === 'Human Review') return 'Human Review';
  if (state === 'Merging') return 'Manual Merge';
  if (state === 'Need Human Input') return 'Doctor';
  return 'Manual Main';
}

function buildCurrentFocus(queueIssues = [], projectWorkerMatch = null) {
  const issue =
    queueIssues.find((item) => item.workerCount === 0 && item.workerStatus !== 'Worker read unavailable') ??
    queueIssues.find((item) => item.workerStatus === 'Worker read unavailable') ??
    queueIssues[0];

  if (!issue) {
    return {
      label: projectWorkerMatch?.label ?? 'No lane work visible',
      title: 'No active Project lane item',
      detail: projectWorkerMatch?.detail ?? 'Project queue and worker session reads have no active lane work.',
      nextSkill: 'Observe',
      tone: projectWorkerMatch?.tone ?? 'neutral'
    };
  }

  return {
    id: issue.id,
    label: `${issue.state} · ${issue.lane}`,
    title: issue.title,
    detail: issue.workerDetail,
    nextSkill: issue.nextSkill,
    tone: issue.workerTone ?? issue.tone ?? 'neutral',
    url: issue.url
  };
}

function buildProjectWorkerMatch(laneProjectIssues = {}, laneWorkers = {}, sessionReadState = { status: 'unknown' }) {
  const laneRows = ['main', 'review', 'merge'].map((lane) => {
    const projectItems = laneProjectIssues[lane] ?? [];
    const workers = laneWorkers[lane] ?? [];
    const projectIds = new Set(projectItems.map((item) => normalizeIssueRef(item.id)).filter(Boolean));
    const workerIds = new Set(workers.map((worker) => normalizeIssueRef(worker.issue)).filter(Boolean));
    const matched = [...projectIds].filter((id) => workerIds.has(id)).length;
    return {
      lane: titleCase(lane),
      project: projectItems.length,
      workers: workers.length,
      matched,
      waiting: Math.max(0, projectIds.size - matched),
      extraWorkers: Math.max(0, workerIds.size - matched)
    };
  });
  const projectTotal = laneRows.reduce((sum, row) => sum + row.project, 0);
  const workerTotal = laneRows.reduce((sum, row) => sum + row.workers, 0);
  const matched = laneRows.reduce((sum, row) => sum + row.matched, 0);
  const waiting = laneRows.reduce((sum, row) => sum + row.waiting, 0);
  const extraWorkers = laneRows.reduce((sum, row) => sum + row.extraWorkers, 0);
  const unavailable = sessionReadState.status === 'unavailable';
  const tone = unavailable || waiting || extraWorkers ? 'warn' : projectTotal || workerTotal ? 'success' : 'neutral';
  const label = unavailable
    ? 'Worker read unavailable'
    : waiting
    ? 'Project waiting'
    : extraWorkers
      ? 'Worker without Project item'
      : projectTotal || workerTotal
        ? 'Project and worker aligned'
        : 'No lane work visible';
  const detail = unavailable
    ? 'Project queue is readable, but worker session surface is unavailable.'
    : waiting
    ? `${waiting} Project item${waiting === 1 ? ' has' : 's have'} no current worker.`
    : extraWorkers
      ? `${extraWorkers} worker${extraWorkers === 1 ? '' : 's'} are not matched to Project lane work.`
      : projectTotal || workerTotal
        ? 'Visible workers match the live Project lane queue.'
        : 'Project lane queue and worker surface are both empty.';

  return {
    label,
    detail,
    tone,
    summary: `${matched}/${projectTotal} matched`,
    projectTotal,
    workerTotal,
    matched,
    waiting,
    extraWorkers,
    sessionReadState,
    lanes: laneRows
  };
}

function buildWorkerMonitor(sessionWorkers: any[] = [], autopilot: any = null, laneProjectIssues: LooseRecord = {}, sessionReadState: any = { status: 'unknown' }) {
  const activeWorkers = (autopilot?.active_issues ?? []).map((issue) => ({
    issue: issue.issue ?? issue.identifier ?? '#?',
    title: issue.title ?? `${titleCase(issue.lane ?? 'lane')} active worker`,
    action: issue.action ?? issue.status ?? 'Active',
    backend: issue.backend ?? 'Shea Symphony CLI',
    session: issue.session ?? issue.run_id ?? 'active',
    elapsed: issue.elapsed ?? 'live',
    evidence: issue.evidence ?? issue.reason ?? 'Active issue surfaced by autopilot.',
    target: issue.target ?? issue.target_state ?? issue.status ?? 'Unknown',
    lane: normalizeSessionLane(issue.lane) ?? 'main',
    source: 'runtime'
  }));
  const runtimeWorkers = [...activeWorkers, ...sessionWorkers.map((worker) => ({ ...worker, source: 'session' }))];
  const lanes = ['main', 'review', 'merge'].map((lane) => {
    const workers = runtimeWorkers.filter((worker) => worker.lane === lane);
    const projectItems = laneProjectIssues[lane] ?? [];
    return {
      lane,
      label: titleCase(lane),
      workers,
      workerCount: workers.length,
      projectCount: projectItems.length,
      tone: workers.length ? 'success' : projectItems.length ? 'warn' : 'neutral'
    };
  });
  const totalWorkers = runtimeWorkers.length;
  const totalProjectItems = lanes.reduce((sum, lane) => sum + lane.projectCount, 0);
  const unavailable = sessionReadState.status === 'unavailable';
  const primaryWorker = runtimeWorkers[0] ?? null;
  const waitingProjectItems = Object.entries(laneProjectIssues).flatMap(([lane, issues]) =>
    ((issues as any[]) ?? []).map((issue) => ({ ...issue, laneKey: lane }))
  );
  const tone = unavailable ? 'warn' : totalWorkers ? 'success' : totalProjectItems ? 'warn' : 'neutral';
  const title = unavailable
    ? 'Worker read unavailable'
    : totalWorkers
    ? `${totalWorkers} worker${totalWorkers === 1 ? '' : 's'} visible`
    : 'No worker visible';
  const detail = unavailable
    ? sessionReadState.detail
    : primaryWorker
    ? `${titleCase(primaryWorker.lane)} lane · ${primaryWorker.issue ?? 'unknown issue'} · ${primaryWorker.elapsed ?? primaryWorker.session}`
    : totalProjectItems
    ? `${totalProjectItems} Project item${totalProjectItems === 1 ? '' : 's'} waiting across lanes.`
    : 'No running worker session or active runtime issue is visible.';

  return {
    title,
    detail,
    tone,
    totalWorkers,
    totalProjectItems,
    primaryWorker,
    lanes,
    waitingProjectItems,
    sessionReadState
  };
}

function normalizeIssueRef(value) {
  const match = String(value ?? '').match(/#?(\d+)/);
  return match ? `#${match[1]}` : null;
}

function queueIssueFromTask(task) {
  const state = normalizeStateName(task.type ?? task.urgency);
  return {
    id: task.id,
    title: task.title,
    state,
    lane: stateToLane(state),
    url: null,
    updatedAt: null,
    assignees: [],
    labels: [],
    evidence: task.evidence,
    recommended: task.recommended,
    tone: task.tone ?? toneForState(state),
    source: task.sourceLabel ?? 'attention'
  };
}

function recommendationForQueueState(state) {
  const normalized = normalizeStateName(state);
  if (normalized === 'Rework') return 'Main lane can resume after checking rework evidence.';
  if (normalized === 'Todo') return 'Run Issue Quality Gate before dispatch.';
  if (normalized === 'Agent Review') return 'Review lane should inspect PR and record independent evidence.';
  if (normalized === 'Human Review') return 'Human operator should review evidence before routing.';
  if (normalized === 'Merging') return 'Merge lane should verify approval and PR mergeability.';
  if (normalized === 'Need Human Input') return 'Inspect issue and diagnostics before choosing a lane.';
  return 'Observe this issue in the Project queue.';
}

function queueIssueSort(left, right) {
  const order = {
    'Need Human Input': 0,
    'Human Review': 1,
    Rework: 2,
    Todo: 3,
    'Agent Review': 4,
    Merging: 5
  };
  return (
    (order[left.state] ?? 99) - (order[right.state] ?? 99) ||
    String(left.id).localeCompare(String(right.id), undefined, { numeric: true })
  );
}

function buildIssueIndex(attentionTasks: any[], laneWorkers: LooseRecord, events: any[], queueIssues: any[] = []) {
  const issues = new Map();

  for (const issue of queueIssues ?? []) {
    const id = issue.id ?? 'Issue';
    const row = ensureIssue(issues, id);
    row.title = issue.title ?? row.title;
    row.state = issue.state ?? row.state;
    row.lane = issue.lane ?? stateToLane(row.state);
    row.evidence = issue.evidence ?? row.evidence;
    row.recommended = issue.recommended ?? row.recommended;
    row.tone = issue.tone ?? row.tone;
    row.sources.add(issue.source ?? 'queue');
  }

  for (const task of attentionTasks) {
    const id = task.id ?? 'Issue';
    const row = ensureIssue(issues, id);
    row.title = task.title ?? row.title;
    row.state = normalizeStateName(task.type ?? task.urgency);
    row.lane = stateToLane(row.state);
    row.evidence = task.evidence ?? row.evidence;
    row.recommended = task.recommended ?? row.recommended;
    row.tone = task.tone ?? row.tone;
    row.sources.add('attention');
  }

  for (const [lane, workers] of Object.entries(laneWorkers ?? {}) as [string, any[]][]) {
    for (const worker of workers ?? []) {
      const id = worker.issue ?? worker.identifier ?? '#?';
      const row = ensureIssue(issues, id);
      row.title = worker.title ?? row.title;
      row.state = worker.target ?? worker.status ?? row.state;
      row.lane = titleCase(lane);
      row.evidence = worker.evidence ?? row.evidence;
      row.recommended = worker.action ?? row.recommended;
      row.sources.add('lane');
    }
  }

  for (const event of events ?? []) {
    const match = String(`${event.title ?? ''} ${event.detail ?? ''}`).match(/#\d+/);
    if (!match) continue;
    const row = ensureIssue(issues, match[0]);
    row.lane = event.lane ?? row.lane;
    row.lastEvent = event.title ?? row.lastEvent;
    row.evidence = event.detail ?? row.evidence;
    row.sources.add('event');
  }

  return [...issues.values()]
    .map((row) => ({ ...row, sources: [...row.sources].join(' + ') }))
    .sort(
      (left, right) =>
        Number(right.tone === 'danger') - Number(left.tone === 'danger') ||
        String(left.id).localeCompare(String(right.id))
    );
}

function skillForTask(task) {
  const state = normalizeStateName(task.type ?? task.urgency);
  const text = `${task.title ?? ''} ${task.reason ?? ''} ${task.recommended ?? ''} ${task.evidence ?? ''}`;
  if (/Human Review|human decision|approve to Merging/i.test(text)) return 'Human Review';
  if (/Agent Review|review evidence|finding/i.test(text)) return 'Manual Review';
  if (state === 'Human Review') return 'Human Review';
  if (state === 'Agent Review') return 'Manual Review';
  if (state === 'Need Human Input' || state === 'Diagnostics') return 'Doctor';
  return 'Manual Main';
}

function severityRank(tone) {
  return { danger: 3, warn: 2, success: 1, neutral: 0 }[tone] ?? 0;
}

function toneForCount(count, sourceTone) {
  if (sourceTone === 'danger') return 'danger';
  if (sourceTone === 'warn' || count > 0) return 'warn';
  return 'neutral';
}

function ensureIssue(issues, id) {
  if (!issues.has(id)) {
    issues.set(id, {
      id,
      title: 'Untitled issue',
      lane: 'Unknown',
      state: 'Unknown',
      evidence: 'No evidence captured yet.',
      recommended: 'Inspect this issue before routing.',
      lastEvent: 'No event surfaced.',
      tone: 'neutral',
      sources: new Set()
    });
  }
  return issues.get(id);
}

function stateToLane(state) {
  if (state === 'Agent Review' || state === 'Human Review') return 'Review';
  if (state === 'Merging') return 'Merge';
  if (state === 'Need Human Input') return 'Human';
  return 'Main';
}

function bump(rows, state, amount = 1, sourceLabel = 'Derived', provenance = 'live') {
  const normalized = normalizeStateName(state);
  if (!rows.has(normalized)) rows.set(normalized, stateRow(normalized, toneForState(normalized)));
  const row = rows.get(normalized);
  row.count += amount;
  if (amount > 0) {
    row.sources.add(sourceLabel);
    row.provenance.add(provenance);
  }
}

function normalizeStateName(value) {
  const normalized = titleCase(value || 'Unknown');
  const aliases = {
    Main: 'In Progress',
    Review: 'Agent Review',
    Merge: 'Merging',
    Parked: 'Need Human Input',
    Diagnostics: 'Need Human Input',
    'Need To Clarify': 'Need to Clarify'
  };
  return aliases[normalized] ?? normalized;
}

function toneForState(state) {
  if (state === 'Need Human Input') return 'danger';
  if (state === 'Human Review' || state === 'Rework') return 'warn';
  if (['In Progress', 'Agent Review', 'Merging'].includes(state)) return 'success';
  return 'neutral';
}

function workersForLane(autopilot, lane) {
  const plan = (autopilot?.lanes ?? []).find((item) => item.lane === lane);
  const selected = plan?.selected_issue
    ? [
        {
          issue: plan.selected_issue,
          title: plan.action ?? `${titleCase(lane)} selected issue`,
          action: plan.action ?? 'Inspect selected issue',
          backend: 'Shea Symphony CLI',
          session: plan.status ?? 'planned',
          elapsed: 'live',
          evidence: plan.reason ?? 'Autopilot plan selected this issue.',
          target: plan.target_state ?? plan.target ?? 'Next lane state'
        }
      ]
    : [];

  const active = (autopilot?.active_issues ?? [])
    .filter((issue) => !issue.lane || issue.lane === lane)
    .map((issue) => ({
      issue: issue.issue ?? issue.identifier ?? '#?',
      title: issue.title ?? `${titleCase(lane)} active issue`,
      action: issue.action ?? issue.status ?? 'Active',
      backend: issue.backend ?? 'Shea Symphony CLI',
      session: issue.session ?? issue.run_id ?? 'active',
      elapsed: issue.elapsed ?? 'live',
      evidence: issue.evidence ?? issue.reason ?? 'Active issue surfaced by autopilot.',
      target: issue.target ?? issue.target_state ?? issue.status ?? 'Unknown'
    }));

  return [...selected, ...active];
}

function firstLine(value) {
  return String(value).split('\n').find(Boolean) ?? String(value);
}

function titleCase(value) {
  return String(value)
    .replace(/[-_]/g, ' ')
    .replace(/\b\w/g, (letter) => letter.toUpperCase());
}

function timeLabel(date) {
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

function labelForCommand(name) {
  const labels = {
    autopilot: 'Autopilot plan',
    doctor: 'Doctor',
    review: 'Review status',
    skills: 'Skills status',
    sessions: 'Session list',
    local: 'Local repo',
    githubQueue: 'GitHub queue'
  };
  return labels[name] ?? titleCase(name);
}

function commandActionForDiagnostic(name) {
  const actions = {
    autopilot: 'autopilot-plan',
    doctor: 'doctor',
    review: 'review-status',
    skills: 'skills-status'
  };
  return actions[name] ?? 'autopilot-plan';
}
