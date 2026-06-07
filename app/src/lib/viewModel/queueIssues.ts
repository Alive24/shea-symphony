import {
  issueRefFromValue,
  isLaneQueueState,
  normalizeIssueRef,
  normalizeStateName,
  stateToLane,
  toneForState
} from './issueState.ts';
import { textFromValue, titleCase } from './text.ts';

export function buildQueueIssues(githubQueue: any, attentionTasks: any[] = []) {
  const fromGithub = (githubQueue?.issues ?? [])
    .map((issue) => {
      const state = normalizeStateName(issue.state);
      const blockedBy = normalizedBlockers(issue.blockedBy ?? issue.blocked_by);
      const unresolvedBlocked = isBlockedMainQueueState(state) && hasUnresolvedBlockers(blockedBy);
      const blockedReason = textFromValue(issue.blockedReason ?? issue.blocked_reason, 'issue has unresolved tracker dependencies');
      return {
        id: issue.identifier,
        title: issue.title,
        state,
        lane: unresolvedBlocked ? 'Blocked' : stateToLane(state),
        url: issue.url,
        updatedAt: issue.updatedAt,
        assignees: issue.assignees ?? [],
        labels: issue.labels ?? [],
        blockedBy,
        blockedReason: blockedBy.length ? blockedReason : null,
        evidence: [
          githubQueue.source ?? 'GitHub queue',
          issue.state,
          unresolvedBlocked ? blockerSummary(blockedBy) : null
        ].filter(Boolean).join(' · '),
        recommended: unresolvedBlocked
          ? 'Blocked by unresolved tracker dependencies; keep out of Main lane selection.'
          : recommendationForQueueState(state),
        tone: unresolvedBlocked ? 'danger' : toneForState(state),
        source: 'githubQueue'
      };
    })
    .filter((issue) => isLaneQueueState(issue.state) && issue.lane !== 'Unknown');

  if (fromGithub.length) return fromGithub.sort(queueIssueSort);

  return (attentionTasks ?? []).map((task) => queueIssueFromTask(task)).sort(queueIssueSort);
}

export function buildAutopilotQueueIssues(autopilot: any) {
  const issues = [];

  for (const lane of autopilot?.lanes ?? []) {
    const selected = lane?.selected_issue;
    const id = issueRefFromValue(selected);
    if (!id) continue;
    const selectedRecord = typeof selected === 'object' && selected !== null ? selected : {};
    const state = normalizeStateName(
      selectedRecord.state ?? selectedRecord.status ?? fallbackStateForLane(lane?.lane)
    );
    issues.push({
      id,
      title: textFromValue(selectedRecord.title ?? lane?.action, `${id} selected by ${titleCase(lane?.lane ?? 'lane')}`),
      state,
      lane: stateToLane(state),
      url: selectedRecord.url ?? selectedRecord.html_url ?? null,
      updatedAt: null,
      assignees: selectedRecord.assignees ?? [],
      labels: selectedRecord.labels ?? [],
      evidence: `Autopilot plan · ${textFromValue(lane?.status, 'selected')} · ${textFromValue(lane?.reason, 'selected issue')}`,
      recommended: recommendationForQueueState(state),
      tone: toneForState(state),
      source: 'autopilotPlan'
    });
  }

  for (const active of autopilot?.active_issues ?? []) {
    const id = issueRefFromValue(active?.issue ?? active?.identifier);
    if (!id) continue;
    const state = normalizeStateName(active?.state ?? fallbackStateForLane(active?.lane));
    issues.push({
      id,
      title: textFromValue(active?.title, `${id} active runtime issue`),
      state,
      lane: stateToLane(state),
      url: active?.url ?? null,
      updatedAt: null,
      assignees: [],
      labels: [],
      evidence: `Runtime state · ${textFromValue(active?.backend, 'unknown backend')} · ${textFromValue(active?.session_id ?? active?.session, 'no visible session')}`,
      recommended: active?.session_id || active?.session
        ? 'Runtime reports a worker session; watch for lane progress.'
        : 'Runtime still points at this issue, but no worker session is visible; recovery has not visibly resumed.',
      tone: active?.session_id || active?.session ? 'success' : 'warn',
      source: 'runtimeState'
    });
  }

  return mergeQueueIssues([], issues).sort(queueIssueSort);
}

export function mergeQueueIssues(primary: any[] = [], secondary: any[] = []) {
  const byId = new Map();
  for (const issue of [...primary, ...secondary]) {
    const id = normalizeIssueRef(issue?.id);
    if (!id) continue;
    const existing = byId.get(id);
    if (!existing) {
      byId.set(id, { ...issue, id });
      continue;
    }
    byId.set(id, {
      ...issue,
      ...existing,
      evidence: [existing.evidence, issue.evidence].filter(Boolean).join(' · '),
      source: [existing.source, issue.source].filter(Boolean).join(' + ')
    });
  }
  return [...byId.values()].sort(queueIssueSort);
}

function fallbackStateForLane(lane: any) {
  const normalized = String(lane ?? '').toLowerCase();
  if (normalized === 'review') return 'Agent Review';
  if (normalized === 'merge') return 'Merging';
  return 'In Progress';
}

function queueIssueFromTask(task: any) {
  const state = normalizeStateName(task.type ?? task.urgency);
  return {
    id: task.id,
    title: task.title,
    state,
    lane: stateToLane(state),
    url: null,
    updatedAt: null,
    assignees: task.assignees ?? [],
    labels: [],
    evidence: task.evidence,
    recommended: task.recommended,
    tone: task.tone ?? toneForState(state),
    source: task.sourceLabel ?? 'attention'
  };
}

function recommendationForQueueState(state: any) {
  const normalized = normalizeStateName(state);
  if (normalized === 'Rework') return 'Main lane can resume after checking rework evidence.';
  if (normalized === 'Todo') return 'Run Issue Quality Gate before dispatch.';
  if (normalized === 'Agent Review') return 'Review lane should inspect PR and record independent evidence.';
  if (normalized === 'Human Review') return 'Human operator should review evidence before routing.';
  if (normalized === 'Merging') return 'Merge lane should verify approval and PR mergeability.';
  if (normalized === 'Need Human Input') return 'Inspect issue and diagnostics before choosing a lane.';
  return 'Observe this issue in the Project queue.';
}

function isBlockedMainQueueState(state: any) {
  return ['Todo', 'Rework'].includes(normalizeStateName(state));
}

function normalizedBlockers(blockedBy: any) {
  return (Array.isArray(blockedBy) ? blockedBy : [])
    .map((blocker) => {
      if (typeof blocker === 'string' || typeof blocker === 'number') {
        return { id: null, identifier: issueRefFromValue(blocker), state: null };
      }
      if (!blocker || typeof blocker !== 'object') return null;
      return {
        id: blocker.id ?? null,
        identifier: issueRefFromValue(blocker.identifier ?? blocker.issue ?? blocker.number ?? blocker.url ?? blocker.id),
        state: blocker.state ? normalizeStateName(blocker.state) : null
      };
    })
    .filter(Boolean);
}

function hasUnresolvedBlockers(blockedBy: any[] = []) {
  return blockedBy.some((blocker) => !isTerminalBlockerState(blocker?.state));
}

function isTerminalBlockerState(state: any) {
  const normalized = normalizeStateName(state);
  return ['Done', 'Closed', 'Merged'].includes(normalized);
}

function blockerSummary(blockedBy: any[] = []) {
  const blockers = blockedBy
    .map((blocker) => {
      const ref = blocker.identifier ?? blocker.id ?? 'unknown blocker';
      return blocker.state ? `${ref} ${blocker.state}` : ref;
    })
    .join(', ');
  return blockers ? `blocked by ${blockers}` : null;
}

function queueIssueSort(left: any, right: any) {
  const order: Record<string, number> = {
    'Need Human Input': 0,
    'Human Review': 1,
    Rework: 2,
    Todo: 3,
    Blocked: 3,
    'Agent Review': 4,
    Merging: 5
  };
  return (
    (order[left.state] ?? 99) - (order[right.state] ?? 99) ||
    String(left.id).localeCompare(String(right.id), undefined, { numeric: true })
  );
}
