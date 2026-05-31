import { normalizeStateName, toneForState } from './issueState.ts';

export function buildStateDistribution(
  autopilot: any,
  laneSummaries: any[],
  attentionTasks: any[],
  githubQueue: any = null,
  githubQueueResult: any = null,
  queueIssues: any[] = []
) {
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

function stateRow(state: string, tone: string) {
  return { state, count: 0, tone, sources: new Set(), provenance: new Set() };
}

function bump(rows: Map<string, any>, state: any, amount = 1, sourceLabel = 'Derived', provenance = 'live') {
  const normalized = normalizeStateName(state);
  if (!rows.has(normalized)) rows.set(normalized, stateRow(normalized, toneForState(normalized)));
  const row = rows.get(normalized);
  row.count += amount;
  if (amount > 0) {
    row.sources.add(sourceLabel);
    row.provenance.add(provenance);
  }
}
