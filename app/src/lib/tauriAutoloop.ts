export type LaneSnapshot = {
  lane: string;
  status: string;
  action?: string | null;
  selected?: string | null;
  sessionId?: string | null;
  target?: string | null;
  workUnitCompleted?: boolean | null;
  completedWorkUnits?: number | null;
  issueRef?: string | null;
  latestResult?: string | null;
  maxConcurrent?: number | null;
  runningCount?: number | null;
  queuedCount?: number | null;
  blockedCount?: number | null;
  idleCount?: number | null;
  completedCount?: number | null;
  recover?: boolean | null;
  updatedAtMs?: number | null;
  latestLine?: string | null;
};

export type AutoloopLine = {
  stream: string;
  line: string;
  atMs: number;
  event?: Record<string, unknown> | null;
};

export type RunLogVerbosity = 'focus' | 'normal' | 'verbose';

export type LoopStateSnapshot = {
  running: boolean;
  stopping: boolean;
  pid?: number | null;
  mode: string;
  workflowPath: string;
  startedAtMs?: number | null;
  stoppedAtMs?: number | null;
  exitCode?: number | null;
  error?: string | null;
  lanes: Record<string, LaneSnapshot>;
  recentLines: AutoloopLine[];
};

export type RuntimeSnapshot = Record<string, unknown>;
export type OperatorOverview = Record<string, unknown>;
export type ReadSurface = Record<string, unknown>;
export type GitHubUserSnapshot = {
  available: boolean;
  login: string;
  name: string;
  email: string;
  avatarUrl: string;
  error: string;
};
export type LaneWorker = {
  issue: string;
  title: string;
  action: string;
  backend: string;
  session: string;
  sessionId?: string | null;
  pid?: number | null;
  updatedAtMs?: number | null;
  elapsed: string;
  lane: string;
  status?: string | null;
  waiting?: boolean;
};

export type StartAutoloopOptions = {
  workflowPath?: string;
  maxIterations?: number;
  once?: boolean;
  continuous?: boolean;
  write?: boolean;
  pollIntervalMs?: number;
  mainMaxConcurrent?: number;
  reviewMaxConcurrent?: number;
  mergeMaxConcurrent?: number;
  signalFormat?: 'json' | 'plain';
};

export type AutoloopEvent =
  | { type: 'started'; payload: unknown }
  | { type: 'line'; payload: AutoloopLine }
  | { type: 'lane'; payload: LaneSnapshot }
  | { type: 'snapshot'; payload: LoopStateSnapshot }
  | { type: 'stopped'; payload: unknown }
  | { type: 'error'; payload: unknown };

const defaultState: LoopStateSnapshot = {
  running: false,
  stopping: false,
  pid: null,
  mode: 'dry-run',
  workflowPath: 'workflows/shea-symphony.md',
  startedAtMs: null,
  stoppedAtMs: null,
  exitCode: null,
  error: null,
  lanes: {
    main: { lane: 'main', status: 'idle' },
    review: { lane: 'review', status: 'idle' },
    merge: { lane: 'merge', status: 'idle' }
  },
  recentLines: []
};

export function isTauriRuntime() {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export function defaultLoopState() {
  return structuredClone(defaultState);
}

export async function getLoopState(): Promise<LoopStateSnapshot> {
  if (!isTauriRuntime()) return defaultLoopState();
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<LoopStateSnapshot>('get_loop_state');
}

export async function getRuntimeSnapshot(): Promise<RuntimeSnapshot | null> {
  if (!isTauriRuntime()) return null;
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<RuntimeSnapshot>('get_runtime_snapshot');
}

export async function getGitHubUser(): Promise<GitHubUserSnapshot> {
  if (!isTauriRuntime()) {
    return {
      available: false,
      login: '',
      name: '',
      email: '',
      avatarUrl: '',
      error: 'GitHub CLI identity is only available in the desktop shell.'
    };
  }
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<GitHubUserSnapshot>('get_github_user');
}

export async function getIssueTimeline(issueRef: string): Promise<Record<string, unknown> | null> {
  if (!isTauriRuntime()) return null;
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<Record<string, unknown>>('get_issue_timeline', { issueRef });
}

export async function getOperatorOverview(force = false, scope = 'full'): Promise<OperatorOverview | null> {
  if (!isTauriRuntime()) return null;
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<OperatorOverview>('get_operator_overview', { options: { force, scope } });
}

export async function getReadSurface(name: string, force = false, allowProjectFallback = false): Promise<ReadSurface | null> {
  if (!isTauriRuntime()) return null;
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<ReadSurface>('get_read_surface', { name, force, allowProjectFallback });
}

export async function getCodexTranscript(issueRef: string, sessionId: string | null = null): Promise<Record<string, unknown> | null> {
  if (!isTauriRuntime()) return null;
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<Record<string, unknown>>('get_codex_transcript', { issueRef, sessionId });
}

export async function openCodexThread(deepLink: string): Promise<void> {
  if (!isTauriRuntime()) {
    window.location.href = deepLink;
    return;
  }
  const { invoke } = await import('@tauri-apps/api/core');
  await invoke('open_codex_thread', { deepLink });
}

export async function openGitHubSource(url: string): Promise<void> {
  if (!isTauriRuntime()) {
    window.open(url, '_blank', 'noopener,noreferrer');
    return;
  }
  const { invoke } = await import('@tauri-apps/api/core');
  await invoke('open_github_source', { url });
}

export async function startAutoloop(options: StartAutoloopOptions = {}): Promise<LoopStateSnapshot> {
  if (!isTauriRuntime()) {
    throw new Error('Tauri runtime is unavailable; open Shea Symphony App in the desktop shell.');
  }
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<LoopStateSnapshot>('start_autoloop', { options });
}

export async function stopAutoloop(): Promise<LoopStateSnapshot> {
  if (!isTauriRuntime()) {
    throw new Error('Tauri runtime is unavailable; open Shea Symphony App in the desktop shell.');
  }
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<LoopStateSnapshot>('stop_autoloop');
}

export async function subscribeAutoloopEvents(handler: (event: AutoloopEvent) => void) {
  if (!isTauriRuntime()) return () => {};
  const { listen } = await import('@tauri-apps/api/event');

  const unlisteners = await Promise.all([
    listen('autoloop:started', (event) => handler({ type: 'started', payload: event.payload })),
    listen<AutoloopLine>('autoloop:line', (event) => handler({ type: 'line', payload: event.payload })),
    listen<LaneSnapshot>('autoloop:lane', (event) => handler({ type: 'lane', payload: event.payload })),
    listen<LoopStateSnapshot>('autoloop:snapshot', (event) => handler({ type: 'snapshot', payload: event.payload })),
    listen('autoloop:stopped', (event) => handler({ type: 'stopped', payload: event.payload })),
    listen('autoloop:error', (event) => handler({ type: 'error', payload: event.payload }))
  ]);

  return () => {
    for (const unlisten of unlisteners) unlisten();
  };
}

export function mergeLaneSnapshot(state: LoopStateSnapshot, lane: LaneSnapshot): LoopStateSnapshot {
  return {
    ...state,
    lanes: {
      ...state.lanes,
      [lane.lane]: lane
    }
  };
}

export function appendAutoloopLine(state: LoopStateSnapshot, line: AutoloopLine): LoopStateSnapshot {
  return {
    ...state,
    recentLines: [...(state.recentLines ?? []), line].slice(-300)
  };
}

export function operatorRunLogLines(
  state: LoopStateSnapshot,
  lines: AutoloopLine[] = state.recentLines ?? [],
  verbosity: RunLogVerbosity = 'normal'
) {
  const startedAt = Number(state.startedAtMs);
  const lowerBound = Number.isFinite(startedAt) ? startedAt - 1000 : null;
  return lines.filter((entry) =>
    (verbosity === 'verbose' || entry.stream === 'stdout')
      && (lowerBound == null || entry.atMs >= lowerBound)
      && isAutoloopLineVisibleAtVerbosity(entry, verbosity)
  );
}

export function isAutoloopLineVisibleAtVerbosity(line: AutoloopLine, verbosity: RunLogVerbosity) {
  if (verbosity === 'verbose') return true;
  if (verbosity === 'focus') return isOperatorVisibleAutoloopLine(line);
  return isNormalRunLogLine(line);
}

export function operatorLoopStatusDetail(payload: Record<string, unknown>) {
  const reasons = arrayFromValue(
    recordValue(payload, 'blocked_reasons') ?? recordValue(payload, 'blockedReasons')
  )
    .map((reason) => textFromValue(reason))
    .filter(Boolean);
  if (reasons.length > 0) return `Blocked: ${reasons.join('; ')}`;
  return textFromValue(recordValue(payload, 'message'), 'Loop status updated.');
}

export function isOperatorVisibleAutoloopLine(line: AutoloopLine) {
  const eventName = textFromValue(recordValue(line.event, 'event'));
  if (eventName === 'autopilot_signal') {
    const payload = recordFromValue(recordValue(line.event, 'payload'));
    return textFromValue(recordValue(payload, 'visibility')) === 'operator';
  }
  if (eventName === 'autopilot_loop_lane') {
    return isOperatorLaneEventLine(line);
  }
  if (eventName === 'autopilot_loop_status') {
    return isOperatorLoopStatusLine(line);
  }
  return false;
}

function isNormalRunLogLine(line: AutoloopLine) {
  if (isOperatorVisibleAutoloopLine(line)) return true;
  const eventName = textFromValue(recordValue(line.event, 'event'));
  if (eventName === 'autopilot_loop_status') {
    const payload = recordFromValue(recordValue(line.event, 'payload'));
    const phase = textFromValue(recordValue(payload, 'phase'));
    const selected = arrayFromValue(recordValue(payload, 'selected_issues'));
    const active = arrayFromValue(recordValue(payload, 'active_issues'));
    const retrying = arrayFromValue(recordValue(payload, 'retrying'));
    return phase === 'running' && (selected.length > 0 || active.length > 0 || retrying.length > 0);
  }
  if (eventName === 'autopilot_loop_result') {
    return !isAutoloopResultNoopEventLine(line);
  }
  if (eventName === 'autopilot_cli_line') {
    return isActionableCliDiagnosticLine(line);
  }
  return false;
}

function isOperatorLaneEventLine(line: AutoloopLine) {
  const payload = recordFromValue(recordValue(line.event, 'payload'));
  const status = textFromValue(recordValue(payload, 'status'));
  const selected = issueRefFromValue(recordValue(payload, 'selected_issue') ?? recordValue(payload, 'selected'));
  const workUnitCompleted = booleanFromRecord(payload, 'work_unit_completed');
  if (workUnitCompleted) return true;
  if (!selected) return false;
  return ['running', 'blocked', 'error', 'retrying', 'waiting'].includes(status);
}

function isOperatorLoopStatusLine(line: AutoloopLine) {
  const payload = recordFromValue(recordValue(line.event, 'payload'));
  const phase = textFromValue(recordValue(payload, 'phase'));
  const blockers = arrayFromValue(recordValue(payload, 'blocked_reasons'));
  return ['blocked', 'error', 'failed'].includes(phase) || blockers.length > 0;
}

function isAutoloopLaneNoopEventLine(line: AutoloopLine) {
  const eventName = textFromValue(recordValue(line.event, 'event'));
  if (eventName !== 'autopilot_loop_lane') return false;
  const payload = recordFromValue(recordValue(line.event, 'payload'));
  const action = textFromValue(recordValue(payload, 'action'));
  const status = textFromValue(recordValue(payload, 'status'));
  const selected = issueRefFromValue(recordValue(payload, 'selected_issue') ?? recordValue(payload, 'selected'));
  const workUnitCompleted = booleanFromRecord(payload, 'work_unit_completed');
  if (selected) return false;
  if (workUnitCompleted) return false;
  return (status === 'running' && action === 'tick_started')
    || (status === 'skipped' && action === 'lane_tick_skipped')
    || status === 'completed';
}

function isAutoloopResultNoopEventLine(line: AutoloopLine) {
  const eventName = textFromValue(recordValue(line.event, 'event'));
  if (eventName !== 'autopilot_loop_result') return false;
  const payload = recordFromValue(recordValue(line.event, 'payload'));
  const completedThisCycle = numberFromValue(recordValue(payload, 'work_units_completed_this_cycle'));
  const lanes = arrayFromValue(recordValue(payload, 'lanes'));
  const hasActionableLane = lanes.some((lane) => {
    const value = recordFromValue(lane);
    const status = textFromValue(recordValue(value, 'status'));
    const selected = issueRefFromValue(recordValue(value, 'selected_issue') ?? recordValue(value, 'selected'));
    const workUnitCompleted = booleanFromRecord(value, 'work_unit_completed');
    return workUnitCompleted || Boolean(selected) || ['error', 'blocked', 'retrying'].includes(status);
  });
  return (completedThisCycle ?? 0) === 0 && !hasActionableLane;
}

function isSkippedIssueDetailLine(line: AutoloopLine) {
  if (line.stream !== 'stdout') return false;
  const eventName = textFromValue(recordValue(line.event, 'event'));
  if (eventName !== 'autopilot_cli_line') return false;
  const payload = recordFromValue(recordValue(line.event, 'payload'));
  const kind = textFromValue(recordValue(payload, 'kind'));
  const raw = textFromValue(recordValue(payload, 'raw') ?? line.line);
  return kind === '-' && /^-\s+\S+\s+#\d+\s+reason=state is not active\b/.test(raw);
}

function isAutoloopLaneIdlePrimitiveLine(line: AutoloopLine) {
  if (line.stream !== 'stdout') return false;
  const eventName = textFromValue(recordValue(line.event, 'event'));
  if (eventName !== 'autopilot_cli_line') return false;
  const payload = recordFromValue(recordValue(line.event, 'payload'));
  const raw = textFromValue(recordValue(payload, 'raw') ?? line.line);
  return /^(merge_once|merge_loop)=stopped reason=no_merging_issue\b/.test(raw)
    || /^review_loop=stopped reason=no_agent_review_issue\b/.test(raw)
    || /^(merge|review)_loop_iteration=\d+\b/.test(raw)
    || /^autopilot_loop_lane\b.*\bstatus=running\b.*\baction=tick_started\b.*\bselected=none\b/.test(raw)
    || /^autopilot_loop_lane\b.*\bstatus=skipped\b.*\baction=lane_tick_skipped\b.*\bselected=none\b/.test(raw)
    || /^autopilot_loop_lane\b.*\bstatus=completed\b.*\bselected=none\b/.test(raw);
}

function isAutoloopRoutineStatusLine(line: AutoloopLine) {
  if (line.stream !== 'stdout') return false;
  const eventName = textFromValue(recordValue(line.event, 'event'));
  if (eventName !== 'autopilot_cli_line') return false;
  const payload = recordFromValue(recordValue(line.event, 'payload'));
  const kind = textFromValue(recordValue(payload, 'kind'));
  const raw = textFromValue(recordValue(payload, 'raw') ?? line.line);
  if (raw === 'SHEA SYMPHONY STATUS') return true;
  if (raw === 'integration gaps:') return true;
  if (kind === 'integration') return true;
  if (/^-\s+GitHub Project v2\b/.test(raw)) return true;
  if (/^canonical_checkout root=.*\bclean=true\b/.test(raw)) return true;
  if (/^canonical_checkout_refresh=(already_current|ff_only|would_ff_only)\b/.test(raw)) return true;
  return false;
}

function isActionableCliDiagnosticLine(line: AutoloopLine) {
  if (line.stream !== 'stdout') return false;
  const payload = recordFromValue(recordValue(line.event, 'payload'));
  const kind = textFromValue(recordValue(payload, 'kind'));
  const raw = textFromValue(recordValue(payload, 'raw') ?? line.line);
  if (!raw) return false;
  if (isAutoloopRoutineStatusLine(line)) return false;
  if (isAutoloopLaneIdlePrimitiveLine(line)) return false;
  if (isSkippedIssueDetailLine(line)) return false;
  if (/^(polling|activity|tokens):\s/.test(raw)) return false;
  if (/^event_log=/.test(raw)) return false;
  if (/^Latest:\s+\w+\s+\|\s+no-issue\s+\|\s+idle\b/.test(raw)) return false;
  if (/^run_loop=stopped reason=no_dispatchable_issue\b/.test(raw)) return false;
  if (kind === 'latest' && /no-issue\s+·\s+idle/.test(raw)) return false;
  return /#\d+/.test(raw)
    || /(^|\s)(reason|error|failure_kind|target_state|pull_request|run_loop_action|tracker_recovery|handoff|blocked)=/.test(raw)
    || /\b(waiting_for_human_input|need_human_input|failed|blocked|stalled|usage_limited)\b/i.test(raw);
}

export function laneWorkerFromAutoloop(
  lane: LaneSnapshot | undefined,
  laneKey: string,
  state: LoopStateSnapshot
): LaneWorker | null {
  if (!lane || lane.status !== 'running') return null;
  if (!state.running && !lane.updatedAtMs) return null;

  const selected = issueRefFromValue(lane.selected);
  if (!selected) return null;

  const action = textFromValue(lane.action, lane.status);
  const target = textFromValue(lane.target, lane.status);
  const waiting = lane.status === 'running' || action === 'tick_started' || action === 'backend';
  const sessionId = workerSessionId(lane, null);
  const backend = workerBackend(lane, laneKey);
  return {
    issue: selected,
    title: selected,
    action,
    backend,
    session: sessionId ?? 'session pending',
    sessionId,
    pid: workerPid(lane, state, backend),
    updatedAtMs: lane.updatedAtMs ?? null,
    elapsed: target,
    lane: laneKey,
    status: lane.status,
    waiting
  };
}

export function laneWorkersFromAutoloopLines(
  state: LoopStateSnapshot,
  laneKey: string
): LaneWorker[] {
  if (!state.running) return [];
  const startedAt = Number(state.startedAtMs);
  const lowerBound = Number.isFinite(startedAt) ? startedAt - 1000 : null;
  const workers = new Map<string, LaneWorker>();

  for (const line of state.recentLines ?? []) {
    if (lowerBound != null && line.atMs < lowerBound) continue;
    if (lineClearsLaneWorkers(line, laneKey)) {
      workers.clear();
      continue;
    }
    const terminalIssue = terminalIssueFromAutoloopLine(line);
    if (terminalIssue) {
      workers.delete(terminalIssue);
      continue;
    }
    const candidate = workerFromAutoloopLine(line, laneKey, state);
    if (candidate) workers.set(candidate.issue, candidate);
  }

  return [...workers.values()];
}

function lineClearsLaneWorkers(line: AutoloopLine, laneKey: string) {
  const eventName = textFromValue(recordValue(line.event, 'event'));
  if (eventName !== 'autopilot_loop_lane') return false;
  const payload = recordFromValue(recordValue(line.event, 'payload'));
  const lane = textFromValue(recordValue(payload, 'lane'));
  if (lane !== laneKey) return false;
  const selected = issueRefFromValue(recordValue(payload, 'selected_issue') ?? recordValue(payload, 'selected'));
  if (selected) return false;
  const status = textFromValue(recordValue(payload, 'status'));
  return status === 'completed' || status === 'idle' || status === 'skipped';
}

function terminalIssueFromAutoloopLine(line: AutoloopLine) {
  const eventName = textFromValue(recordValue(line.event, 'event'));
  if (eventName === 'autopilot_loop_lane') {
    const payload = recordFromValue(recordValue(line.event, 'payload'));
    const issue = issueFromLanePayload(payload);
    const status = textFromValue(recordValue(payload, 'status'));
    const workUnitCompleted = booleanFromRecord(payload, 'work_unit_completed');
    if (issue && workUnitCompleted && status === 'completed') return issue;
  }
  if (eventName !== 'autopilot_cli_line') return null;
  const payload = recordFromValue(recordValue(line.event, 'payload'));
  const fields = recordFromValue(recordValue(payload, 'fields'));
  const issue = issueFromLanePayload(fields);
  if (!issue) return null;
  const state = textFromValue(recordValue(fields, 'state')).toLowerCase();
  const result = textFromValue(recordValue(fields, 'result')).toLowerCase();
  const outcome = textFromValue(recordValue(fields, 'outcome')).toLowerCase();
  const mergeAction = textFromValue(recordValue(fields, 'merge_loop_action') ?? recordValue(fields, 'merging_pool_action'));
  if (isRunLoopResumePreflightArchive(line)) return issue;
  if (['done', 'closed'].includes(state)) return issue;
  if (['merged', 'closed'].includes(result) && outcome === 'applied') return issue;
  if (mergeAction === 'closed_issue' && outcome === 'applied') return issue;
  return null;
}

function workerFromAutoloopLine(
  line: AutoloopLine,
  laneKey: string,
  state: LoopStateSnapshot
): LaneWorker | null {
  const event = line.event;
  const eventName = textFromValue(recordValue(event, 'event'));
  const payload = recordFromValue(recordValue(event, 'payload'));
  const fields = recordFromValue(recordValue(payload, 'fields'));

  if (eventName === 'autopilot_loop_lane') {
    const lane = textFromValue(recordValue(payload, 'lane'));
    if (lane !== laneKey) return null;
    const issue = issueFromLanePayload(payload);
    if (!issue) return null;
    const status = textFromValue(recordValue(payload, 'status'), 'running');
    const latestResult = textFromValue(recordValue(payload, 'latest_result') ?? recordValue(payload, 'latestResult'));
    return liveWorker(
      issue,
      laneKey,
      state,
      latestResult || textFromValue(recordValue(payload, 'action'), status),
      status,
      payload,
      line.atMs
    );
  }

  if (eventName === 'autopilot_signal') {
    const lane = textFromValue(recordValue(payload, 'lane'));
    if (lane !== laneKey) return null;
    const issue = issueFromLanePayload(payload);
    if (!issue) return null;
    const status = textFromValue(recordValue(payload, 'status'), 'running');
    const action = textFromValue(recordValue(payload, 'action'), status);
    if (status === 'idle' || action === 'skip') return null;
    return liveWorker(issue, laneKey, state, action, status, payload, line.atMs);
  }

  if (eventName !== 'autopilot_cli_line') return null;

  const parsedLane = textFromValue(recordValue(fields, 'lane'));
  const kind = textFromValue(recordValue(payload, 'kind'));
  const inferredLane = parsedLane || (kind.startsWith('run_loop') ? 'main' : '');
  if (inferredLane !== laneKey) return null;

  const issue = issueFromLanePayload(fields);
  if (!issue) return null;
  if (isRunLoopResumePreflightArchive(line)) return null;
  const status = textFromValue(recordValue(fields, 'status'), 'running');
  const action = textFromValue(recordValue(fields, 'action'), textFromValue(recordValue(fields, 'run_loop_action'), kind || status));
  if (status === 'idle' || action === 'skip') return null;
  return liveWorker(issue, laneKey, state, action, status, fields, line.atMs);
}

function isRunLoopResumePreflightArchive(line: AutoloopLine) {
  const eventName = textFromValue(recordValue(line.event, 'event'));
  if (eventName !== 'autopilot_cli_line') return false;
  const payload = recordFromValue(recordValue(line.event, 'payload'));
  const kind = textFromValue(recordValue(payload, 'kind'));
  if (kind !== 'run_loop_resume_preflight') return false;
  const fields = recordFromValue(recordValue(payload, 'fields'));
  return textFromValue(recordValue(fields, 'action')) === 'archive';
}

function liveWorker(
  issue: string,
  laneKey: string,
  state: LoopStateSnapshot,
  action: string,
  status: string,
  sessionSource: Record<string, unknown> = {},
  updatedAtMs: number | null = null
): LaneWorker {
  const sessionId = workerSessionId(sessionSource, null);
  const backend = workerBackend(sessionSource, laneKey);
  return {
    issue,
    title: issue,
    action,
    backend,
    session: sessionId ?? 'session pending',
    sessionId,
    pid: workerPid(sessionSource, state, backend),
    updatedAtMs,
    elapsed: status,
    lane: laneKey,
    status,
    waiting: true
  };
}

function workerBackend(source: Record<string, unknown>, laneKey: string) {
  const backend = textFromValue(
    recordValue(source, 'backend')
      ?? recordValue(source, 'backend_kind')
      ?? recordValue(source, 'backendKind')
      ?? recordValue(source, 'backend_source')
      ?? recordValue(source, 'backendSource')
  );
  return displayBackend(backend) || unknownBackendForLane(laneKey);
}

function displayBackend(backend: string) {
  const normalized = backend.toLowerCase();
  if (!normalized) return '';
  if (normalized === 'codex-app-server' || normalized === 'codex app-server') return 'Codex app-server';
  if (normalized === 'gemini-cli' || normalized === 'gemini') return 'Gemini CLI';
  if (normalized === 'codex') return 'Codex';
  if (normalized === 'tmux') return 'tmux';
  return backend;
}

function unknownBackendForLane(laneKey: string) {
  if (laneKey === 'main') return 'Main worker';
  if (laneKey === 'review') return 'Review worker';
  if (laneKey === 'merge') return 'Merge worker';
  return 'Worker';
}

function workerPid(source: Record<string, unknown>, state: LoopStateSnapshot, backend: string) {
  const pid = numberFromValue(recordValue(source, 'pid') ?? recordValue(source, 'process_id') ?? recordValue(source, 'processId'));
  if (pid != null) return pid;
  return backend === 'Codex app-server' ? state.pid ?? null : null;
}

function workerSessionId(source: Record<string, unknown>, fallback: string | null) {
  const session = textFromValue(
    recordValue(source, 'session_id')
      ?? recordValue(source, 'sessionId')
      ?? recordValue(source, 'backend_session_id')
      ?? recordValue(source, 'backendSessionId')
      ?? recordValue(source, 'session')
      ?? recordValue(source, 'run_id')
      ?? recordValue(source, 'runId')
  );
  return session || fallback;
}

function issueFromLanePayload(payload: Record<string, unknown>) {
  return issueRefFromValue(
    recordValue(payload, 'selected_issue')
      ?? recordValue(payload, 'selectedIssue')
      ?? recordValue(payload, 'selected')
      ?? recordValue(payload, 'issue_ref')
      ?? recordValue(payload, 'issueRef')
      ?? recordValue(payload, 'issue')
      ?? recordValue(payload, 'identifier')
  );
}

function recordValue(value: unknown, key: string): unknown {
  return recordFromValue(value)[key];
}

function recordFromValue(value: unknown): Record<string, unknown> {
  return value != null && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {};
}

function arrayFromValue(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function numberFromValue(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

function booleanFromRecord(value: Record<string, unknown>, key: string) {
  return recordValue(value, key) === true || recordValue(value, snakeToCamel(key)) === true;
}

function snakeToCamel(value: string) {
  return value.replace(/_([a-z])/g, (_, letter: string) => letter.toUpperCase());
}

function issueRefFromValue(value: unknown) {
  if (value == null || value === '' || value === 'none') return null;
  if (typeof value === 'number') return `#${value}`;
  if (typeof value === 'string') {
    const trimmed = value.trim();
    if (!trimmed || trimmed === 'none') return null;
    const match = trimmed.match(/#?(\d+)/);
    return match ? `#${match[1]}` : trimmed;
  }
  if (typeof value === 'object') {
    const record = value as Record<string, unknown>;
    return issueRefFromValue(
      record.identifier ??
        record.issue ??
        record.id ??
        record.number ??
        record.url ??
        record.html_url ??
        record.title
    );
  }
  return String(value);
}

function textFromValue(value: unknown, fallback = '') {
  if (value == null || value === '' || value === 'none') return fallback;
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  if (typeof value === 'object') {
    const record = value as Record<string, unknown>;
    return textFromValue(
      record.title ??
        record.name ??
        record.label ??
        record.action ??
        record.status ??
        record.state ??
        record.identifier ??
        record.issue,
      fallback
    );
  }
  return fallback;
}
