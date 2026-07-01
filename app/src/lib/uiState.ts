import { writable } from 'svelte/store';
import { defaultWorkspaceProfile, type WorkspaceProfile } from './tauriAutoloop.ts';

export type CliLogEntry = {
  id: number;
  at: string;
  surface: string;
  phase: string;
  status: string;
  detail: string;
  args: string[];
  raw?: unknown;
  durationMs: number | null;
};

export type ActiveTarget = {
  workflowPath?: string;
  repository?: string;
  workspacePath?: string;
};

export const DATA_MODE_KEY = 'shea-data-mode';
export const FIXTURE_OVERVIEW_KEY = 'shea-fixture-overview';
export const HANDOFF_TARGET_KEY = 'shea-handoff-target';
export const ACTIVE_TARGET_KEY = 'shea-active-target';
export const DATA_MODE_CHANGE_EVENT = 'shea-data-mode-change';
export const HANDOFF_TARGET_CHANGE_EVENT = 'shea-handoff-target-change';
export const REFRESH_REQUEST_EVENT = 'shea-refresh-request';
export const START_DRY_RUN_EVENT = 'shea-start-dry-run-autoloop';
export const START_WRITE_EVENT = 'shea-start-write-autoloop';
export const STOP_AUTOLOOP_EVENT = 'shea-stop-autoloop';
export const OPEN_AUTOLOOP_LOGS_EVENT = 'shea-open-autoloop-logs';
export const HANDOFF_TARGETS = [
  { id: 'codex-app', label: 'Codex App', icon: 'codex' },
  { id: 'claude-code', label: 'Claude Code', icon: 'claude' },
  { id: 'gemini-cli', label: 'Gemini CLI', icon: 'gemini' }
];

export const defaultHandoffTargetStore = writable('codex-app');
export const workspaceProfileStore = writable<WorkspaceProfile>(defaultWorkspaceProfile());
export const autoloopStateStore = writable(null);
export const cliLogStore = writable<CliLogEntry[]>([]);
export const autoloopControlStore = writable({
  tauriAvailable: false,
  busy: false,
  running: false,
  mode: 'dry-run',
  workflowPath: 'workflows/shea-symphony.md',
  targetRoot: '',
  latestLine: 'No recent autoloop result',
  laneMaxSummary: ''
});
export const refreshStatusStore = writable({
  running: false,
  remaining: 0,
  startedAt: null as string | null,
  finishedAt: null as string | null,
  source: 'idle',
  detail: 'Idle'
});

let cliLogSequence = 0;

export function recordCliLog(entry: {
  surface?: string;
  phase?: string;
  status?: string;
  detail?: string;
  args?: string[];
  raw?: unknown;
  durationMs?: number | null;
}) {
  const nextEntry = {
    id: ++cliLogSequence,
    at: new Date().toISOString(),
    surface: entry.surface ?? 'cli',
    phase: entry.phase ?? 'event',
    status: entry.status ?? 'info',
    detail: entry.detail ?? '',
    args: entry.args ?? [],
    raw: entry.raw,
    durationMs: entry.durationMs ?? null
  };
  cliLogStore.update((logs) => [nextEntry, ...logs].slice(0, 200));
  return nextEntry.id;
}

export function updateCliLog(
  id: number,
  entry: {
    surface?: string;
    phase?: string;
    status?: string;
    detail?: string;
    args?: string[];
    raw?: unknown;
    durationMs?: number | null;
  }
) {
  cliLogStore.update((logs) =>
    logs.map((log) =>
      log.id === id
        ? {
            ...log,
            surface: entry.surface ?? log.surface,
            phase: entry.phase ?? log.phase,
            status: entry.status ?? log.status,
            detail: entry.detail ?? log.detail,
            args: entry.args ?? log.args,
            raw: entry.raw ?? log.raw,
            durationMs: entry.durationMs ?? log.durationMs
          }
        : log
    )
  );
}

export function getDataMode() {
  const storage = browserStorage();
  if (!storage) return 'live';
  return storage.getItem(DATA_MODE_KEY) === 'fixture' ? 'fixture' : 'live';
}

export function setDataMode(mode: 'live' | 'fixture') {
  const storage = browserStorage();
  if (!storage) return;
  storage.setItem(DATA_MODE_KEY, mode);
  window.dispatchEvent(new CustomEvent(DATA_MODE_CHANGE_EVENT, { detail: { mode } }));
}

export function getDefaultHandoffTarget() {
  const storage = browserStorage();
  if (!storage) return 'codex-app';
  const saved = storage.getItem(HANDOFF_TARGET_KEY);
  return HANDOFF_TARGETS.some((target) => target.id === saved) ? saved : 'codex-app';
}

export function setDefaultHandoffTarget(targetId: string) {
  const storage = browserStorage();
  if (!storage) return;
  const nextTarget = HANDOFF_TARGETS.some((target) => target.id === targetId) ? targetId : 'codex-app';
  storage.setItem(HANDOFF_TARGET_KEY, nextTarget);
  defaultHandoffTargetStore.set(nextTarget);
  window.dispatchEvent(new CustomEvent(HANDOFF_TARGET_CHANGE_EVENT, { detail: { target: nextTarget } }));
}

export function getActiveTarget(): ActiveTarget {
  const storage = browserStorage();
  if (!storage) return {};
  try {
    const parsed = JSON.parse(storage.getItem(ACTIVE_TARGET_KEY) ?? '{}');
    if (!parsed || typeof parsed !== 'object') return {};
    return {
      workflowPath: nonemptyString(parsed.workflowPath),
      repository: nonemptyString(parsed.repository),
      workspacePath: nonemptyString(parsed.workspacePath)
    };
  } catch (_) {
    return {};
  }
}

export function setActiveTarget(target: ActiveTarget) {
  const storage = browserStorage();
  if (!storage) return;
  const next = {
    workflowPath: nonemptyString(target.workflowPath),
    repository: nonemptyString(target.repository),
    workspacePath: nonemptyString(target.workspacePath)
  };
  if (!next.workflowPath && !next.repository && !next.workspacePath) {
    storage.removeItem(ACTIVE_TARGET_KEY);
    return;
  }
  storage.setItem(ACTIVE_TARGET_KEY, JSON.stringify(next));
}

function nonemptyString(value: unknown) {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined;
}

export function resetFixtureOverview() {
  const storage = browserStorage();
  if (!storage) return;
  storage.removeItem(FIXTURE_OVERVIEW_KEY);
  window.dispatchEvent(new CustomEvent(DATA_MODE_CHANGE_EVENT, { detail: { mode: getDataMode(), reset: true } }));
}

export function browserStorage(): Storage | null {
  if (typeof window === 'undefined') return null;
  return window.localStorage ?? null;
}
