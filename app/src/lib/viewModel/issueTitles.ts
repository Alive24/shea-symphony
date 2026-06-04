import { normalizeIssueRef } from './issueState.ts';

const TRANSIENT_TITLE_LABELS = new Set([
  'waiting for agent response',
  'project read unavailable',
  'tick_started',
  'reviewing',
  'lane_tick_completed',
  'lane_tick_started',
  'readiness_blocked',
  'blocked',
  'running',
  'completed',
  'failed',
  'error',
  'unknown',
  'untitled issue'
]);

export function nonTransientIssueTitle(value: any, issueRef: string | null = null) {
  if (typeof value !== 'string') return null;
  const title = value.trim();
  if (!title) return null;
  const normalizedTitleIssue = normalizeIssueRef(title);
  if (issueRef && normalizedTitleIssue === issueRef) return null;
  if (TRANSIENT_TITLE_LABELS.has(title.toLowerCase())) return null;
  return title;
}

export function issueIdentityTitle(
  issueRef: string | null,
  candidates: any[] = [],
  fallback = 'Issue'
) {
  for (const candidate of candidates) {
    const title = nonTransientIssueTitle(candidate, issueRef);
    if (title) return title;
  }
  return issueRef ?? fallback;
}
