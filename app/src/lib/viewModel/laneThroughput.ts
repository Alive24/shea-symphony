import { normalizeIssueRef } from './issueState.ts';
import { issueIdentityTitle } from './issueTitles.ts';
import { titleCase } from './text.ts';

const LANE_KEYS = ['main', 'review', 'merge'];

export function buildLaneThroughputBoard({
  queueIssues = [],
  laneWorkers = {},
  liveWorkersByLane = {},
  laneSnapshots = {},
  issueTitleById = new Map(),
  fullLoading = false
}: {
  queueIssues?: any[];
  laneWorkers?: Record<string, any[]>;
  liveWorkersByLane?: Record<string, any[]>;
  laneSnapshots?: Record<string, any>;
  issueTitleById?: Map<string, string>;
  fullLoading?: boolean;
} = {}) {
  const workersByLane = reconcileWorkersByIssue(liveWorkersByLane, laneWorkers);
  const activeIssueIds = new Set(
    Object.values(workersByLane).flat().map((worker) => normalizeIssueRef(worker.issue)).filter(Boolean)
  );
  return LANE_KEYS.map((laneKey) => {
    const label = titleCase(laneKey);
    const snapshot = laneSnapshots?.[laneKey] ?? {};
    const workers = workersByLane[laneKey] ?? [];
    const queued = (queueIssues ?? []).filter((issue) => issue.lane === label);
    const workerIssues = workers.map((worker, index) => workerIssueRow(worker, index, issueTitleById));
    const pickedIssueIds = new Set(workerIssues.map((issue) => normalizeIssueRef(issue.id)).filter(Boolean));
    const waitingIssues = queued
      .filter((issue) => {
        const id = normalizeIssueRef(issue.id);
        return !pickedIssueIds.has(id) && !activeIssueIds.has(id);
      })
      .map((issue) => ({
        kind: 'queued',
        id: issue.id,
        title: issueTitle(issue, issueTitleById),
        meta: issueMeta(issue),
        tone: issue.tone,
        workerNumber: null,
        waiting: false
      }));
    const laneResult = laneStatusRow(snapshot);
    const runningCount = workerIssues.filter((issue) => issue.kind === 'picked').length;
    const queuedCount = countFromSnapshot(snapshot, 'queuedCount', waitingIssues.length);
    const blockedCount = countFromSnapshot(snapshot, 'blockedCount', laneResult?.kind === 'blocked' ? 1 : 0);
    const completedCount = countFromSnapshot(
      snapshot,
      'completedCount',
      workerIssues.filter((issue) => issue.kind === 'completed').length + (laneResult?.kind === 'completed' ? 1 : 0)
    );
    const idleCount = countFromSnapshot(
      snapshot,
      'idleCount',
      runningCount || queuedCount || blockedCount || completedCount ? 0 : 1
    );
    const issues = [...workerIssues, ...waitingIssues, ...(laneResult ? [laneResult] : [])];
    const status = laneHeaderStatus({ runningCount, queuedCount, blockedCount, completedCount });
    return {
      laneKey,
      label,
      issues,
      status,
      runningCount,
      pickedCount: runningCount,
      queuedCount,
      blockedCount,
      idleCount,
      completedCount,
      maxConcurrent: snapshot?.maxConcurrent ?? null,
      latest: laneLatest(snapshot, fullLoading),
      tone: laneTone({ runningCount, queuedCount, blockedCount, completedCount, idleCount })
    };
  });
}

function reconcileWorkersByIssue(liveWorkersByLane: Record<string, any[]> = {}, laneWorkers: Record<string, any[]> = {}) {
  const byIssue = new Map();
  let order = 0;
  for (const [sourcePriority, source] of [[1, laneWorkers], [2, liveWorkersByLane]] as [number, Record<string, any[]>][]) {
    for (const laneKey of LANE_KEYS) {
      for (const worker of source?.[laneKey] ?? []) {
        const issue = normalizeIssueRef(worker.issue);
        const key = issue ?? `${laneKey}:${worker.issue ?? worker.action ?? order}`;
        const timestamp = Number(worker.updatedAtMs ?? worker.atMs);
        const score = Number.isFinite(timestamp) ? timestamp : 0;
        const candidate = {
          laneKey,
          worker: { ...worker, lane: worker.lane ?? laneKey },
          score,
          sourcePriority,
          order: order++
        };
        const existing = byIssue.get(key);
        if (
          !existing
          || candidate.score > existing.score
          || (candidate.score === existing.score && candidate.sourcePriority > existing.sourcePriority)
          || (
            candidate.score === existing.score
            && candidate.sourcePriority === existing.sourcePriority
            && candidate.order > existing.order
          )
        ) {
          byIssue.set(key, candidate);
        }
      }
    }
  }

  const lanes = Object.fromEntries(LANE_KEYS.map((laneKey) => [laneKey, []]));
  for (const { laneKey, worker } of byIssue.values()) {
    lanes[laneKey].push(worker);
  }
  return Object.fromEntries(Object.entries(lanes).map(([laneKey, workers]) => [laneKey, uniqueWorkers(workers)]));
}

function uniqueWorkers(workers: any[]) {
  const seen = new Set();
  return (workers ?? []).filter((worker) => {
    const key = normalizeIssueRef(worker.issue) ?? `${worker.lane ?? 'lane'}:${worker.issue ?? worker.action ?? ''}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function workerIssueRow(worker: any, index: number, issueTitleById: Map<string, string>) {
  const normalizedWorkerIssue = normalizeIssueRef(worker.issue);
  const status = String(worker.status ?? '').toLowerCase();
  const completed = status === 'completed';
  const blocked = status === 'blocked' || status === 'error' || status === 'failed';
  return {
    kind: completed ? 'completed' : blocked ? 'blocked' : 'picked',
    id: normalizedWorkerIssue ?? worker.issue ?? `worker-${index + 1}`,
    title: workerDisplayTitle(worker, issueTitleById),
    meta: workerRuntimeMeta(worker),
    tone: blocked ? 'danger' : completed ? 'success' : 'success',
    workerNumber: index + 1,
    waiting: worker.waiting === true || status === 'running'
  };
}

function workerRuntimeMeta(worker: any) {
  return `${displayWorkerBackend(worker.backend)} · ${workerRuntimeState(worker)}`;
}

function displayWorkerBackend(value: any) {
  const backend = String(value ?? 'worker').trim();
  const normalized = backend.toLowerCase();
  if (normalized === 'codex-app-server' || normalized === 'codex app-server') return 'Codex app-server';
  if (normalized === 'gemini-cli' || normalized === 'gemini') return 'Gemini CLI';
  if (normalized === 'codex') return 'Codex';
  if (normalized === 'tmux') return 'tmux';
  return backend || 'worker';
}

function workerRuntimeState(worker: any) {
  const status = String(worker.status ?? '').toLowerCase();
  if (['failed', 'error'].includes(status)) return 'failed';
  if (status === 'blocked') return 'blocked';
  if (status === 'completed') return 'completed';
  return hasWorkerSession(worker) ? 'active' : 'starting';
}

function hasWorkerSession(worker: any) {
  const session = String(worker.sessionId ?? worker.session ?? '').trim().toLowerCase();
  return Boolean(session && session !== 'session pending');
}

function workerDisplayTitle(worker: any, titles: Map<string, string>) {
  const issueRef = normalizeIssueRef(worker.issue);
  const projectTitle = issueRef ? titles.get(issueRef) : null;
  return issueIdentityTitle(issueRef, [projectTitle, worker.title], 'Worker session');
}

function issueTitle(issue: any, titles: Map<string, string>) {
  const issueRef = normalizeIssueRef(issue.id);
  const projectTitle = issueRef ? titles.get(issueRef) : null;
  return issueIdentityTitle(issueRef, [projectTitle, issue.title], 'Issue');
}

function issueMeta(issue: any) {
  const state = cleanMetaPart(issue.state);
  const workerStatus = cleanMetaPart(issue.workerStatus);
  const workerDetail = cleanMetaPart(issue.workerDetail);
  const recommended = cleanMetaPart(issue.recommended);
  const evidenceParts = evidenceMetaParts(issue.evidence, state);
  const parts = [
    quietDefaultState(state) ? null : state,
    quietDefaultWorkerStatus(workerStatus) ? null : workerStatus,
    quietDefaultWorkerDetail(workerDetail) ? null : workerDetail,
    ...evidenceParts,
    quietDefaultRecommendation(recommended, state) ? null : recommended
  ].filter(Boolean);
  return dedupeMetaParts(parts).join(' · ');
}

function cleanMetaPart(value: any) {
  if (typeof value !== 'string') return null;
  const text = value.trim();
  return text ? text : null;
}

function evidenceMetaParts(value: any, state: string | null) {
  const text = cleanMetaPart(value);
  if (!text) return [];
  const parts = text.split(' · ').map((part) => part.trim()).filter(Boolean);
  if (isRoutineAutopilotReadyEvidence(parts)) return [];
  const filtered = parts.filter((part) => {
    if (sameMeta(part, state)) return false;
    return !defaultEvidenceSourceLabels.has(part.toLowerCase());
  });
  return filtered.length ? filtered : [];
}

function isRoutineAutopilotReadyEvidence(parts: string[]) {
  const lowerParts = parts.map((part) => part.toLowerCase());
  return lowerParts.includes('autoloop plan') && lowerParts.includes('ready');
}

function quietDefaultState(state: string | null) {
  return sameMeta(state, 'Todo');
}

function quietDefaultWorkerStatus(status: string | null) {
  return sameMeta(status, 'No worker visible');
}

function quietDefaultWorkerDetail(detail: string | null) {
  return sameMeta(detail, 'Project is waiting in this lane; no current worker session is visible.');
}

function quietDefaultRecommendation(recommended: string | null, state: string | null) {
  if (!recommended) return true;
  if (sameMeta(state, 'Todo') && sameMeta(recommended, 'Run Issue Quality Gate before dispatch.')) return true;
  if (sameMeta(state, 'Agent Review') && sameMeta(recommended, 'Review lane should inspect PR and record independent evidence.')) return true;
  if (sameMeta(state, 'Human Review') && sameMeta(recommended, 'Human operator should review evidence before routing.')) return true;
  if (sameMeta(state, 'Merging') && sameMeta(recommended, 'Merge lane should verify approval and PR mergeability.')) return true;
  if (sameMeta(state, 'Need Human Input') && sameMeta(recommended, 'Inspect issue and diagnostics before choosing a lane.')) return true;
  return false;
}

function dedupeMetaParts(parts: (string | null)[]) {
  const seen = new Set();
  return parts.filter((part) => {
    if (!part) return false;
    const key = part.toLowerCase();
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function sameMeta(left: string | null, right: string | null) {
  return String(left ?? '').trim().toLowerCase() === String(right ?? '').trim().toLowerCase();
}

const defaultEvidenceSourceLabels = new Set([
  'github queue',
  'project state',
  'runtime state',
  'autopilot plan'
]);

function laneStatusRow(snapshot: any) {
  const status = String(snapshot?.status ?? '').toLowerCase();
  if (status === 'blocked' || status === 'error' || status === 'failed') {
    return statusEventRow(snapshot, 'blocked', 'danger', 'Lane blocked');
  }
  return null;
}

function statusEventRow(snapshot: any, kind: string, tone: string, fallbackTitle: string) {
  return {
    kind,
    id: titleCase(kind),
    title: snapshot?.action ?? fallbackTitle,
    meta: [snapshot?.target, snapshot?.status].filter(Boolean).join(' · '),
    tone,
    workerNumber: null,
    waiting: false
  };
}

function countFromSnapshot(snapshot: any, key: string, fallback: number) {
  const value = Number(snapshot?.[key]);
  return Number.isFinite(value) ? value : fallback;
}

function laneLatest(snapshot: any, fullLoading: boolean) {
  if (snapshot?.action || snapshot?.status) {
    return [snapshot.action, snapshot.status].filter(Boolean).join(' · ');
  }
  return fullLoading ? 'Loading CLI readback...' : 'No recent lane event.';
}

function laneHeaderStatus({
  runningCount,
  queuedCount,
  blockedCount,
  completedCount
}: {
  runningCount: number;
  queuedCount: number;
  blockedCount: number;
  completedCount: number;
}) {
  if (blockedCount) return 'blocked';
  if (runningCount) return 'running';
  if (queuedCount) return 'queued';
  if (completedCount) return 'complete';
  return 'idle';
}

function laneTone({
  runningCount,
  queuedCount,
  blockedCount,
  completedCount,
  idleCount
}: {
  runningCount: number;
  queuedCount: number;
  blockedCount: number;
  completedCount: number;
  idleCount: number;
}) {
  if (blockedCount) return 'danger';
  if (runningCount) return 'success';
  if (queuedCount) return 'warn';
  if (completedCount) return 'success';
  if (idleCount) return 'neutral';
  return 'neutral';
}
