import { normalizeStateName } from './issueState.ts';

const skillByState: Record<string, string> = {
  'Need to Clarify': 'shea-symphony-issue-forge',
  'Need Human Input': 'shea-symphony-doctor',
  'Human Review': 'shea-symphony-human-review',
  Rework: 'shea-symphony-manual-main',
  Todo: 'shea-symphony-manual-main',
  'In Progress': 'shea-symphony-manual-main',
  'Agent Review': 'shea-symphony-manual-review',
  Merging: 'shea-symphony-manual-merge'
};

export function handoffSkillForIssue(issue: Record<string, any>) {
  const state = normalizeStateName(issue?.state);
  return skillByState[state] ?? 'shea-symphony-doctor';
}

export function buildHandoffPrompt(issue: Record<string, any>) {
  const state = normalizeStateName(issue?.state);
  const issueId = String(issue?.id ?? 'unknown issue');
  const skill = handoffSkillForIssue(issue);
  const title = String(issue?.title ?? '').trim();
  const recommended = String(issue?.recommended ?? '').trim();
  const evidence = String(issue?.evidence ?? '').trim();
  const workerStatus = String(issue?.workerStatus ?? '').trim();
  const workerDetail = String(issue?.workerDetail ?? '').trim();

  return [
    `Use the ${skill} skill for ${issueId}.`,
    '',
    'Context',
    `- Issue: ${[issueId, title].filter(Boolean).join(' ')}`,
    `- State: ${state}`,
    issue?.lane ? `- Lane: ${issue.lane}` : null,
    issue?.category ? `- Category: ${issue.category}` : null,
    workerStatus ? `- Worker status: ${workerStatus}` : null,
    workerDetail ? `- Worker detail: ${workerDetail}` : null,
    recommended ? `- Recommended next read: ${recommended}` : null,
    evidence ? `- Evidence: ${evidence}` : null,
    issue?.url ? `- URL: ${issue.url}` : null,
    '',
    'Instructions',
    '- Read current Project issue state before acting.',
    '- Preserve Shea lane boundaries and do not mutate Project state without explicit approval.',
    '- Keep the operator-facing readback concise and source-grounded.'
  ]
    .filter((line) => line != null)
    .join('\n');
}
