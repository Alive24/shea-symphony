export type LaneSnapshot = {
  lane: string;
  status: string;
  action?: string | null;
  selected?: string | null;
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
    recentLines: [...(state.recentLines ?? []), line].slice(-200)
  };
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
  return {
    issue: selected,
    title: selected,
    action,
    backend: `Tauri ${state.mode}`,
    session: state.pid ? `pid ${state.pid}` : 'autoloop',
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
    const candidate = workerFromAutoloopLine(line, laneKey, state);
    if (candidate) workers.set(candidate.issue, candidate);
  }

  return [...workers.values()];
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
    const issue = issueRefFromValue(recordValue(payload, 'selected_issue') ?? recordValue(payload, 'selected'));
    if (!issue) return null;
    const status = textFromValue(recordValue(payload, 'status'), 'running');
    const latestResult = textFromValue(recordValue(payload, 'latest_result') ?? recordValue(payload, 'latestResult'));
    return liveWorker(
      issue,
      laneKey,
      state,
      latestResult || textFromValue(recordValue(payload, 'action'), status),
      status
    );
  }

  if (eventName !== 'autopilot_cli_line') return null;

  const parsedLane = textFromValue(recordValue(fields, 'lane'));
  const kind = textFromValue(recordValue(payload, 'kind'));
  const inferredLane = parsedLane || (kind.startsWith('run_loop') ? 'main' : '');
  if (inferredLane !== laneKey) return null;

  const issue = issueRefFromValue(recordValue(fields, 'issue'));
  if (!issue) return null;
  const status = textFromValue(recordValue(fields, 'status'), 'running');
  const action = textFromValue(recordValue(fields, 'action'), textFromValue(recordValue(fields, 'run_loop_action'), kind || status));
  if (status === 'idle' || action === 'skip') return null;
  return liveWorker(issue, laneKey, state, action, status);
}

function liveWorker(
  issue: string,
  laneKey: string,
  state: LoopStateSnapshot,
  action: string,
  status: string
): LaneWorker {
  return {
    issue,
    title: issue,
    action,
    backend: `Tauri ${state.mode}`,
    session: state.pid ? `pid ${state.pid}` : 'autoloop',
    elapsed: status,
    lane: laneKey,
    status,
    waiting: true
  };
}

function recordValue(value: unknown, key: string): unknown {
  return recordFromValue(value)[key];
}

function recordFromValue(value: unknown): Record<string, unknown> {
  return value != null && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {};
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
