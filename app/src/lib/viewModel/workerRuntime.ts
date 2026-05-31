import { normalizeIssueRef, normalizeStateName } from './issueState.ts';
import { normalizeSessionLane } from './sessionParsers.ts';
import { issueRefFromValue } from './issueState.ts';
import { textFromValue, titleCase } from './text.ts';

type LooseRecord = Record<string, any>;

export function annotateQueueIssuesWithWorkers(
  queueIssues: any[],
  laneWorkers: LooseRecord,
  sessionReadState: any = { status: 'unknown' }
) {
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

export function buildCurrentFocus(queueIssues: any[] = [], projectWorkerMatch: any = null) {
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

export function buildProjectWorkerMatch(
  laneProjectIssues: LooseRecord = {},
  laneWorkers: LooseRecord = {},
  sessionReadState: any = { status: 'unknown' }
) {
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

export function buildWorkerMonitor(
  sessionWorkers: any[] = [],
  autopilot: any = null,
  laneProjectIssues: LooseRecord = {},
  sessionReadState: any = { status: 'unknown' }
) {
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

export function workersForLane(autopilot: any, lane: string) {
  const active = (autopilot?.active_issues ?? [])
    .filter((issue) => !issue.lane || issue.lane === lane)
    .filter((issue) => issue.session || issue.session_id || issue.run_id || issue.status === 'running')
    .map((issue) => ({
      issue: issueRefFromValue(issue.issue ?? issue.identifier) ?? '#?',
      title: textFromValue(issue.title, `${titleCase(lane)} active issue`),
      action: textFromValue(issue.action ?? issue.status, 'Active'),
      backend: textFromValue(issue.backend, 'Shea Symphony CLI'),
      session: textFromValue(issue.session ?? issue.session_id ?? issue.run_id, 'active'),
      elapsed: textFromValue(issue.elapsed, 'live'),
      evidence: textFromValue(issue.evidence ?? issue.reason, 'Active issue surfaced by autopilot.'),
      target: textFromValue(issue.target ?? issue.target_state ?? issue.status, 'Unknown')
    }));

  return active;
}

function skillForQueueIssue(issue: any) {
  const state = normalizeStateName(issue.state);
  if (state === 'Agent Review') return 'Manual Review';
  if (state === 'Human Review') return 'Human Review';
  if (state === 'Merging') return 'Manual Merge';
  if (state === 'Need Human Input') return 'Doctor';
  return 'Manual Main';
}
