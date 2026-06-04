export type IssueCommentLifecycleEvent = {
  phase: string;
  label: string;
  time: unknown;
  sortTime?: unknown;
  detail: string;
  url?: string;
  source: string;
};

export function buildIssueCommentLifecycleEvents(snapshot: any, issue: any): IssueCommentLifecycleEvent[] {
  if (!snapshot?.available) return [];
  const events: IssueCommentLifecycleEvent[] = [];
  if (snapshot.issue?.createdAt) {
    events.push({
      phase: 'Backlog',
      label: 'Issue created on GitHub',
      time: snapshot.issue.createdAt,
      detail: snapshot.issue.title ?? issue?.title ?? '',
      url: snapshot.issue.url ?? issue?.url,
      source: 'github.issue'
    });
  }
  for (const comment of snapshot.comments ?? []) {
    const event = issueCommentLifecycleEvent(comment, snapshot.issue ?? issue);
    if (event) events.push(event);
  }
  events.push(...needHumanInputEvents(snapshot));
  if (snapshot.issue?.closedAt) {
    const mergeTime = latestPhaseTime(events, 'Merge');
    const closedAtMs = dateMs(snapshot.issue.closedAt);
    const mergeMs = dateMs(mergeTime);
    events.push({
      phase: 'Done',
      label: 'Issue closed on GitHub',
      time: snapshot.issue.closedAt,
      sortTime: mergeMs && closedAtMs && closedAtMs <= mergeMs ? mergeMs + 1 : snapshot.issue.closedAt,
      detail: snapshot.issue.state ? `Issue state: ${snapshot.issue.state}` : '',
      url: snapshot.issue.url ?? issue?.url,
      source: 'github.issue'
    });
  }
  return events;
}

export function issueCommentLifecycleEvent(comment: any, issue: any = null): IssueCommentLifecycleEvent | null {
  const body = String(comment?.body ?? '');
  const phase = phaseFromIssueComment(body);
  if (!phase) return null;
  const heading = firstMarkdownHeading(body);
  return {
    phase,
    label: heading ?? `${phase} timeline comment`,
    time: comment?.createdAt,
    detail: commentDetail(body, heading, issue),
    url: comment?.url,
    source: 'github.issue.comments'
  };
}

function phaseFromIssueComment(body: string) {
  const text = body.toLowerCase();
  if (text.includes('## shea symphony workpad')) return 'Main';
  if (text.includes('## agent review handoff') || text.includes('### agent review handoff invariant')) return 'Handoff';
  if (/\bpromot(?:e|ed|ion)\b/.test(text) || text.includes('promoted into') || text.includes('promoted to')) return 'Promoted';
  if (text.includes('## shea symphony doctor triage') || /\bdoctor\b/.test(text)) return 'Doctor';
  if (text.includes('## shea symphony human review decision') || text.includes('approve for merging')) return 'Human Review';
  if (text.includes('## shea symphony agent review run') || text.includes('## manual agent review evidence') || text.includes('agent review')) return 'Agent Review';
  if (text.includes('## shea symphony rework run') || /\brework\b/.test(text)) return 'Rework';
  if (text.includes('## shea symphony merge run') || /\bmerge lane\b/.test(text) || /\bmerging\b/.test(text)) return 'Merge';
  if (text.includes('shea symphony') && text.includes('timeline')) return 'Timeline';
  return null;
}

function latestPhaseTime(events: IssueCommentLifecycleEvent[], phase: string) {
  let latest: unknown = null;
  let latestMs = 0;
  for (const event of events) {
    if (event.phase !== phase) continue;
    const ms = dateMs(event.sortTime ?? event.time);
    if (ms > latestMs) {
      latestMs = ms;
      latest = event.sortTime ?? event.time;
    }
  }
  return latest;
}

function needHumanInputEvents(snapshot: any) {
  const doctorComments = (snapshot.comments ?? []).filter((comment: any) =>
    phaseFromIssueComment(String(comment?.body ?? '')) === 'Doctor'
    && /need human input/i.test(String(comment?.body ?? ''))
  );
  const statusEvents = (snapshot.timelineEvents ?? [])
    .filter((event: any) => event.event === 'project_v2_item_status_changed')
    .filter((event: any) => dateMs(event.createdAt));
  const issueUrl = snapshot.issue?.url;
  const events: IssueCommentLifecycleEvent[] = [];
  for (const doctor of doctorComments) {
    const doctorMs = dateMs(doctor.createdAt);
    const status = latestStatusBefore(statusEvents, doctorMs);
    if (!status) continue;
    events.push({
      phase: 'Need Human Input',
      label: 'Moved to Need Human Input',
      time: status.createdAt,
      detail: 'Project status changed to Need Human Input, inferred from Doctor triage evidence.',
      url: issueUrl,
      source: 'github.issue.timeline'
    });
  }
  return events;
}

function latestStatusBefore(statusEvents: any[], beforeMs: number) {
  let winner: any = null;
  let winnerMs = 0;
  for (const event of statusEvents) {
    const ms = dateMs(event.createdAt);
    if (ms && ms < beforeMs && ms > winnerMs) {
      winner = event;
      winnerMs = ms;
    }
  }
  return winner;
}

function dateMs(value: unknown) {
  if (!value) return 0;
  if (typeof value === 'number') return value;
  const parsed = Date.parse(String(value));
  return Number.isNaN(parsed) ? 0 : parsed;
}

function firstMarkdownHeading(body: string) {
  for (const line of body.split(/\r?\n/)) {
    const match = line.match(/^#{1,4}\s+(.+?)\s*$/);
    if (match) return cleanMarkdown(match[1]);
  }
  return null;
}

function commentDetail(body: string, heading: string | null, issue: any = null) {
  const lines = body.split(/\r?\n/);
  const preferred = preferredDetailLine(lines);
  if (preferred) return preferred;
  for (const line of body.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (shouldSkipDetailLine(trimmed, heading, issue)) continue;
    if (heading && cleanMarkdown(trimmed) === heading) continue;
    return truncate(cleanMarkdown(trimmed.replace(/^[-*]\s+/, '')), 180);
  }
  return '';
}

function preferredDetailLine(lines: string[]) {
  const preferredLabels = [
    'decision',
    'result',
    'reason',
    'target state after review routing',
    'target state after explicit confirmation',
    'target state after merge routing',
    'evidence summary',
    'finding',
    'repair applied'
  ];
  for (const label of preferredLabels) {
    const match = lines
      .map((line) => line.trim())
      .find((line) => normalizedFieldLabel(line) === label);
    if (match) return truncate(cleanMarkdown(match.replace(/^[-*]\s+/, '')), 180);
  }
  return '';
}

function normalizedFieldLabel(line: string) {
  const match = line.match(/^[-*]\s*([^:]+):/);
  return match ? cleanMarkdown(match[1]).toLowerCase() : '';
}

function shouldSkipDetailLine(line: string, heading: string | null, issue: any) {
  if (!line || line.startsWith('<!--') || /^#{1,4}\s+/.test(line)) return true;
  const cleaned = cleanMarkdown(line.replace(/^[-*]\s+/, ''));
  if (heading && cleaned === heading) return true;
  if (/^generated at:/i.test(cleaned)) return true;
  if (/^issue:/i.test(cleaned)) return true;
  if (issue?.title && cleaned.includes(String(issue.title))) return true;
  return false;
}

function cleanMarkdown(value: string) {
  return value
    .replace(/`([^`]+)`/g, '$1')
    .replace(/\[([^\]]+)\]\([^)]+\)/g, '$1')
    .replace(/\*\*([^*]+)\*\*/g, '$1')
    .replace(/__([^_]+)__/g, '$1')
    .trim();
}

function truncate(value: string, max: number) {
  return value.length > max ? `${value.slice(0, max - 1)}...` : value;
}
