import { normalizeStateName, stateToLane } from './issueState.ts';
import { titleCase } from './text.ts';

type LooseRecord = Record<string, any>;

export function buildIssueIndex(
  attentionTasks: any[],
  laneWorkers: LooseRecord,
  events: any[],
  queueIssues: any[] = []
) {
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

function ensureIssue(issues: Map<string, any>, id: string) {
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
