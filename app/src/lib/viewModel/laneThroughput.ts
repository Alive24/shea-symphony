import { normalizeIssueRef } from './issueState.ts';
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
  return LANE_KEYS.map((laneKey) => {
    const label = titleCase(laneKey);
    const snapshot = laneSnapshots?.[laneKey] ?? {};
    const workers = uniqueWorkers([...(liveWorkersByLane?.[laneKey] ?? []), ...(laneWorkers?.[laneKey] ?? [])]);
    const queued = (queueIssues ?? []).filter((issue) => issue.lane === label);
    const workerIssues = workers.map((worker, index) => workerIssueRow(worker, index, issueTitleById));
    const pickedIssueIds = new Set(workerIssues.map((issue) => normalizeIssueRef(issue.id)).filter(Boolean));
    const waitingIssues = queued
      .filter((issue) => !pickedIssueIds.has(normalizeIssueRef(issue.id)))
      .map((issue) => ({
        kind: 'queued',
        id: issue.id,
        title: issue.title,
        meta: `${issue.state} · Next Skill: ${issue.nextSkill}`,
        tone: issue.tone,
        workerNumber: null,
        waiting: false
      }));
    const laneResult = laneStatusRow(snapshot, workerIssues.length + waitingIssues.length);
    const runningCount = countFromSnapshot(snapshot, 'runningCount', workerIssues.filter((issue) => issue.kind === 'picked').length);
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
    const statusText = [
      `running ${runningCount}`,
      `queued ${queuedCount}`,
      `blocked ${blockedCount}`,
      `idle ${idleCount}`,
      `done ${completedCount}`
    ].join(' · ');
    return {
      laneKey,
      label,
      issues,
      runningCount,
      pickedCount: runningCount,
      queuedCount,
      blockedCount,
      idleCount,
      completedCount,
      maxConcurrent: snapshot?.maxConcurrent ?? null,
      latest: laneLatest(snapshot, fullLoading),
      statusText,
      tone: laneTone({ runningCount, queuedCount, blockedCount, completedCount, idleCount })
    };
  });
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
    meta: `${worker.action ?? 'Active'} · ${worker.backend ?? 'worker'} · ${worker.session ?? worker.elapsed ?? 'session'}`,
    tone: blocked ? 'danger' : completed ? 'success' : 'success',
    workerNumber: index + 1,
    waiting: worker.waiting === true || status === 'running'
  };
}

function workerDisplayTitle(worker: any, titles: Map<string, string>) {
  const issueRef = normalizeIssueRef(worker.issue);
  const projectTitle = issueRef ? titles.get(issueRef) : null;
  if (projectTitle) return projectTitle;
  if (worker.title && normalizeIssueRef(worker.title) !== issueRef) return worker.title;
  if (worker.action && worker.action !== 'tick_started') return worker.action;
  return 'Waiting for agent response';
}

function laneStatusRow(snapshot: any, visibleIssueCount: number) {
  const status = String(snapshot?.status ?? '').toLowerCase();
  if (status === 'blocked' || status === 'error' || status === 'failed') {
    return statusEventRow(snapshot, 'blocked', 'danger', 'Lane blocked');
  }
  if (status === 'completed') {
    return statusEventRow(snapshot, 'completed', 'success', 'Latest lane result completed');
  }
  if (status === 'idle' && visibleIssueCount === 0) {
    return statusEventRow(snapshot, 'idle', 'neutral', 'Lane idle');
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
