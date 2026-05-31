import { titleCase } from './text.ts';

type KeyValueFields = Record<string, any>;

export function parseSessionCount(text: any) {
  const value = String(text ?? '').trim();
  if (!value) return null;
  if (/agent_session_list=unavailable/.test(value)) return null;
  if (/agent_session_list=none/.test(value)) return 0;
  const countMatch = value.match(/(?:count|session_count)=(\d+)/);
  if (countMatch) return Number(countMatch[1]);
  const sessionLines = value.split('\n').filter((line) => line.trim() && !/agent_session_list=/.test(line));
  return sessionLines.length || null;
}

export function parseSessionReadState(text: any) {
  const value = String(text ?? '').trim();
  if (!value) return { status: 'unknown', detail: 'Session list did not return text.' };
  if (/agent_session_list=unavailable/.test(value)) {
    const reason = value.match(/reason=([^\s]+)/)?.[1] ?? 'unknown';
    return { status: 'unavailable', detail: `Worker session surface unavailable: ${reason}.` };
  }
  if (/agent_session_list=none/.test(value)) {
    return { status: 'none', detail: 'No foreground agent sessions are visible.' };
  }
  return { status: 'readable', detail: 'Worker session surface is readable.' };
}

export function parseSessionWorkers(text: any) {
  const value = String(text ?? '').trim();
  if (!value || /agent_session_list=none/.test(value)) return [];
  return value
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.startsWith('agent_session '))
    .map((line) => sessionWorkerFromFields(parseKeyValueLine(line)))
    .filter(Boolean);
}

function parseKeyValueLine(line: string) {
  const fields: KeyValueFields = {};
  const pattern = /(\w+)=("([^"]*)"|[^\s]+)/g;
  let match;
  while ((match = pattern.exec(line))) {
    fields[match[1]] = match[3] ?? match[2];
  }
  return fields;
}

function sessionWorkerFromFields(fields: KeyValueFields) {
  const session = fields.session ?? fields.session_name ?? fields.name;
  if (!session) return null;
  const lane = normalizeSessionLane(fields.lane ?? laneFromSessionName(session));
  if (!lane) return null;
  const issue = fields.issue ?? fields.issue_identifier ?? issueFromSessionName(session);
  const status = fields.status ?? (fields.attached === '1' ? 'attached' : 'running');
  return {
    issue: issue ?? session,
    title: fields.title ?? `${titleCase(lane)} worker session`,
    action: fields.action ?? 'Session registered in local worker surface.',
    backend: fields.backend ?? 'tmux/session',
    session,
    elapsed: status,
    evidence: fields.evidence ?? fields.attach_command ?? `session=${session}`,
    target: fields.target ?? status,
    lane,
    url: null
  };
}

export function normalizeSessionLane(value: any) {
  const normalized = String(value ?? '').toLowerCase();
  if (normalized.includes('main')) return 'main';
  if (normalized.includes('review')) return 'review';
  if (normalized.includes('merge') || normalized.includes('merging')) return 'merge';
  return null;
}

function laneFromSessionName(session: any) {
  const value = String(session ?? '').toLowerCase();
  const match = value.match(/(?:^|-)(main|review|merge|merging)(?:-|$)/);
  return match?.[1];
}

function issueFromSessionName(session: any) {
  const value = String(session ?? '');
  const match = value.match(/(?:^|-)#?(\d+)(?:-|$)/);
  return match ? `#${match[1]}` : null;
}
