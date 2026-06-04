export type TranscriptTone = 'neutral' | 'success' | 'warn' | 'danger';
export type TranscriptEventKind =
  | 'user'
  | 'assistant'
  | 'final'
  | 'tool_call'
  | 'tool_output'
  | 'diagnostic'
  | 'usage'
  | 'raw';

export type TranscriptEvent = {
  id: string;
  kind: TranscriptEventKind;
  title: string;
  body: string;
  detail?: string;
  timestamp?: string;
  tone: TranscriptTone;
  raw?: unknown;
};

export type TranscriptParseResult = {
  status: 'available' | 'partial' | 'empty' | 'malformed';
  events: TranscriptEvent[];
  rawRecords: unknown[];
  malformedLines: number;
  summary: {
    userTurns: number;
    assistantTurns: number;
    toolCalls: number;
    diagnostics: number;
    tokenUsage: string | null;
    rawRecords: number;
    readableEvents: number;
    unsupportedRecords: number;
  };
};

export type HeartbeatSummary = {
  state: 'running' | 'stale' | 'stopped' | 'unavailable';
  tone: TranscriptTone;
  label: string;
  lastHeartbeatMs: number | null;
  lastHeartbeatAge: string;
  latestLaneEvent: string;
  lane: string;
  issue: string | null;
};

const staleAfterMs = 2 * 60 * 1000;

export function classifyHeartbeat(
  loopState: any,
  laneKey: string,
  issueRef: string | null = null,
  nowMs = Date.now()
): HeartbeatSummary {
  const lane = loopState?.lanes?.[laneKey];
  const lastHeartbeatMs = numberOrNull(lane?.updatedAtMs ?? latestLineAt(loopState));
  const ageMs = lastHeartbeatMs == null ? null : Math.max(0, nowMs - lastHeartbeatMs);
  const latestLaneEvent = usefulLaneEvent(loopState, laneKey, issueRef) ?? lane?.latestLine ?? lane?.action ?? 'No lane event visible.';

  if (!loopState) {
    return heartbeat('unavailable', null, 'unavailable', 'Autoloop state is unavailable.', latestLaneEvent, laneKey, issueRef);
  }
  if (!loopState.running) {
    return heartbeat('stopped', lastHeartbeatMs, ageLabel(ageMs), 'Loop stopped', latestLaneEvent, laneKey, issueRef);
  }
  if (lastHeartbeatMs == null) {
    return heartbeat('unavailable', null, 'unavailable', 'Heartbeat unavailable', latestLaneEvent, laneKey, issueRef);
  }
  if (ageMs != null && ageMs > staleAfterMs) {
    return heartbeat('stale', lastHeartbeatMs, ageLabel(ageMs), 'Heartbeat stale', latestLaneEvent, laneKey, issueRef);
  }
  return heartbeat('running', lastHeartbeatMs, ageLabel(ageMs), 'Loop running', latestLaneEvent, laneKey, issueRef);
}

export function parseCodexTranscriptJsonl(text: unknown): TranscriptParseResult {
  const lines = String(text ?? '')
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
  const events: TranscriptEvent[] = [];
  const rawRecords: unknown[] = [];
  let malformedLines = 0;

  lines.forEach((line, index) => {
    let record: any;
    try {
      record = JSON.parse(line);
      rawRecords.push(record);
    } catch {
      malformedLines += 1;
      events.push({
        id: `malformed-${index}`,
        kind: 'diagnostic',
        title: 'Malformed JSONL line',
        body: line.slice(0, 240),
        tone: 'warn'
      });
      return;
    }

    const mapped = transcriptEventFromRecord(record, index);
    if (mapped && !isDuplicateEvent(events[events.length - 1], mapped)) events.push(mapped);
  });

  const summary = {
    userTurns: events.filter((event) => event.kind === 'user').length,
    assistantTurns: events.filter((event) => event.kind === 'assistant' || event.kind === 'final').length,
    toolCalls: events.filter((event) => event.kind === 'tool_call').length,
    diagnostics: events.filter((event) => event.kind === 'diagnostic').length,
    tokenUsage: latestUsage(events),
    rawRecords: rawRecords.length,
    readableEvents: events.length,
    unsupportedRecords: Math.max(0, rawRecords.length - events.length)
  };
  const status = !lines.length
    ? 'empty'
    : malformedLines
      ? events.length > malformedLines
        ? 'partial'
        : 'malformed'
      : 'available';
  return { status, events, rawRecords, malformedLines, summary };
}

export function transcriptUnavailable(reason: string) {
  return {
    status: 'unavailable',
    reason: reason || 'No local Codex transcript candidate was found.',
    localOnly: true,
    candidates: [],
    content: '',
    path: null
  };
}

function transcriptEventFromRecord(record: any, index: number): TranscriptEvent | null {
  const protocol = protocolMessage(record);
  if (protocol) return transcriptEventFromProtocol(protocol, index, record);

  const wrapper = text(record?.type);
  const payload = record?.payload;
  const item = record?.item ?? payload?.item ?? payload ?? record?.message ?? record;
  const type = text(item?.type ?? record?.event ?? record?.kind ?? record?.type);
  const role = text(item?.role ?? record?.role);
  const status = text(item?.status ?? record?.status);
  const name = text(item?.name ?? item?.tool_name ?? record?.name ?? record?.tool_name);
  const timestamp = eventTimestamp(record);
  const usage = item?.usage ?? item?.response?.usage ?? record?.usage ?? record?.token_usage ?? record?.response?.usage ?? item?.info?.total_token_usage ?? item?.info?.last_token_usage;

  if (wrapper === 'session_meta') {
    return event(index, 'diagnostic', 'Session metadata', summarizeObject({
      id: item?.id,
      cwd: item?.cwd,
      model: item?.model ?? item?.model_provider,
      source: item?.source,
      cli: item?.cli_version
    }), 'neutral', 'session_meta', record, timestamp);
  }
  if (wrapper === 'turn_context') return null;
  if (wrapper === 'event_msg') {
    if (type === 'user_message') {
      return event(index, 'user', 'User', previewText(item?.message), 'neutral', type, record, timestamp);
    }
    if (type === 'agent_message') {
      const phase = text(item?.phase);
      const final = phase === 'final_answer';
      return event(index, final ? 'final' : 'assistant', final ? 'Final answer' : 'Assistant', previewText(item?.message), final ? 'success' : 'neutral', phase || type, record, timestamp);
    }
    if (type === 'token_count') {
      const tokenUsage = item?.info?.total_token_usage ?? item?.info?.last_token_usage ?? item?.info;
      return event(index, 'usage', 'Token usage', usageSummary(tokenUsage), 'neutral', type, record, timestamp);
    }
    if (/error|cancel|interrupt|input|required|failed|task_started/.test(`${type} ${status}`)) {
      const title = type === 'task_started' ? 'Task started' : diagnosticTitle(type, status);
      return event(index, 'diagnostic', title, previewText(item?.message ?? item?.error ?? item), /error|failed/.test(`${type} ${status}`) ? 'danger' : 'warn', type || status || undefined, record, timestamp);
    }
    return null;
  }

  if (usage && !role && !item?.content && !record?.content) {
    return event(index, 'usage', 'Token usage', usageSummary(usage), 'neutral', undefined, record, timestamp);
  }
  if (role && role !== 'user' && role !== 'assistant') return null;

  if (role === 'user' || type === 'user_message') {
    return event(index, 'user', 'User', contentText(item || record), 'neutral', type || undefined, record, timestamp);
  }
  if (role === 'assistant' || type === 'assistant_message' || (type === 'message' && !role)) {
    const body = contentText(item || record);
    const final = /final|answer/i.test(type) || record?.is_final === true;
    return event(index, final ? 'final' : 'assistant', final ? 'Final answer' : 'Assistant', body, final ? 'success' : 'neutral', status || undefined, record, timestamp);
  }
  if (/final|response\.completed/.test(type)) {
    const body = contentText(item || record) || text(record?.response?.output_text);
    if (body) return event(index, 'final', 'Final answer', body, 'success', status || undefined, record, timestamp);
  }
  if (type.includes('function_call_output') || type.includes('tool_output') || item?.output) {
    return event(index, 'tool_output', name || 'Tool output', previewText(item?.output ?? record?.output ?? record?.result), status === 'failed' ? 'danger' : 'neutral', status || undefined, record, timestamp);
  }
  if (type.includes('function_call') || type.includes('tool_call') || item?.arguments || item?.input) {
    const args = item?.arguments ?? item?.input ?? record?.arguments ?? record?.input;
    return event(index, 'tool_call', name || 'Tool call', summarizeArguments(args), 'neutral', status || 'started', record, timestamp);
  }
  if (/error|cancel|interrupt|input|required|failed/.test(`${type} ${status}`)) {
    return event(index, 'diagnostic', diagnosticTitle(type, status), previewText(record?.error ?? record?.message ?? record), /error|failed/.test(`${type} ${status}`) ? 'danger' : 'warn', status || undefined, record, timestamp);
  }
  if (usage) {
    return event(index, 'usage', 'Token usage', usageSummary(usage), 'neutral', undefined, record, timestamp);
  }
  return null;
}

function protocolMessage(record: any) {
  if (!record?.direction || !record?.line) return null;
  try {
    const message = JSON.parse(String(record.line));
    return {
      direction: text(record.direction),
      method: text(message?.method),
      id: message?.id,
      params: message?.params,
      result: message?.result,
      error: message?.error
    };
  } catch {
    return {
      direction: text(record.direction),
      method: 'protocol.raw',
      params: { line: record.line },
      result: null,
      error: null
    };
  }
}

function transcriptEventFromProtocol(protocol: any, index: number, raw: unknown): TranscriptEvent | null {
  const method = text(protocol.method);
  const params = protocol.params ?? {};
  const item = params.item ?? {};

  if (protocol.error) {
    return event(index, 'diagnostic', method || 'Protocol error', previewText(protocol.error), 'danger', protocol.direction, raw, eventTimestamp(raw));
  }
  if (method === 'turn/start') {
    const input = Array.isArray(params.input)
      ? params.input.map((entry: any) => text(entry?.text ?? entry?.content ?? entry)).filter(Boolean).join('\n\n')
      : contentText(params.input);
    return input ? event(index, 'user', 'User', input, 'neutral', 'turn/start', raw, eventTimestamp(raw)) : null;
  }
  if (method === 'item/started' && item.type === 'commandExecution') {
    return event(index, 'tool_call', item.command || 'Command execution', summarizeArguments({ cwd: item.cwd, command: item.command }), 'neutral', item.status || 'started', raw, eventTimestamp(raw));
  }
  if (method === 'item/completed' && item.type === 'commandExecution') {
    return event(index, 'tool_output', item.command || 'Command output', previewText(item.aggregatedOutput ?? item.output ?? item.exitCode), item.status === 'failed' || item.exitCode ? 'danger' : 'neutral', item.status || 'completed', raw, eventTimestamp(raw));
  }
  if (method === 'item/completed' && item.type === 'agentMessage') {
    const phase = text(item.phase);
    const final = phase === 'final_answer';
    return event(index, final ? 'final' : 'assistant', final ? 'Final answer' : 'Assistant', previewText(item.text), final ? 'success' : 'neutral', phase || 'agentMessage', raw, eventTimestamp(raw));
  }
  if (method === 'thread/tokenUsage/updated') {
    const usage = params.tokenUsage?.total ?? params.tokenUsage?.last ?? params.tokenUsage;
    return event(index, 'usage', 'Token usage', usageSummary(usage), 'neutral', undefined, raw, eventTimestamp(raw));
  }
  if (method === 'configWarning') {
    return event(index, 'diagnostic', 'Config warning', previewText(params.summary ?? params.details ?? params), 'warn', method, raw, eventTimestamp(raw));
  }
  if (/input|required|cancel|error|failed/.test(`${method} ${previewText(params, 120)}`)) {
    return event(index, 'diagnostic', diagnosticTitle(method, ''), previewText(params), /error|failed/.test(method) ? 'danger' : 'warn', method, raw, eventTimestamp(raw));
  }
  return null;
}

function event(index: number, kind: TranscriptEventKind, title: string, body: string, tone: TranscriptTone, detail?: string, raw?: unknown, timestamp?: string): TranscriptEvent {
  return {
    id: `${kind}-${index}`,
    kind,
    title,
    body: body || '(empty)',
    detail,
    timestamp,
    tone,
    raw
  };
}

function isDuplicateEvent(previous: TranscriptEvent | undefined, next: TranscriptEvent) {
  return previous?.kind === next.kind
    && previous?.timestamp === next.timestamp
    && previous?.body === next.body;
}

function heartbeat(
  state: HeartbeatSummary['state'],
  lastHeartbeatMs: number | null,
  lastHeartbeatAge: string,
  label: string,
  latestLaneEvent: string,
  lane: string,
  issue: string | null
): HeartbeatSummary {
  const tone = state === 'running' ? 'success' : state === 'stale' ? 'warn' : state === 'stopped' ? 'neutral' : 'warn';
  return { state, tone, label, lastHeartbeatMs, lastHeartbeatAge, latestLaneEvent, lane, issue };
}

function usefulLaneEvent(loopState: any, laneKey: string, issueRef: string | null) {
  const lines = [...(loopState?.recentLines ?? [])].reverse();
  for (const entry of lines) {
    const event = entry?.event;
    const payload = event?.payload ?? {};
    const raw = text(payload.raw ?? entry.line);
    const lane = text(payload.lane ?? payload.fields?.lane ?? event?.lane);
    if (lane && lane !== laneKey) continue;
    const issue = normalizeIssueRef(payload.issue ?? payload.fields?.issue ?? payload.selected_issue ?? payload.selected);
    if (issueRef && issue && issue !== normalizeIssueRef(issueRef)) continue;
    if (isNoisyLine(raw)) continue;
    if (event?.event === 'autopilot_signal' && payload.message) return text(payload.message);
    if (raw) return raw;
  }
  return null;
}

function latestLineAt(loopState: any) {
  const lines = loopState?.recentLines ?? [];
  return lines.length ? lines[lines.length - 1]?.atMs : null;
}

function isNoisyLine(raw: string) {
  return raw === 'SHEA SYMPHONY STATUS'
    || raw === 'integration gaps:'
    || /^-\s+GitHub Project v2\b/.test(raw)
    || /^canonical_checkout/.test(raw)
    || /^polling:/.test(raw)
    || /^activity:/.test(raw)
    || /^tokens:/.test(raw)
    || /^event_log=/.test(raw)
    || /reason=state is not active\b/.test(raw)
    || /reason=no_(merging|agent_review|dispatchable)_issue\b/.test(raw);
}

function normalizeIssueRef(value: unknown) {
  if (value == null || value === '' || value === 'none' || value === 'no-issue') return null;
  if (typeof value === 'object') {
    const record = value as Record<string, unknown>;
    return normalizeIssueRef(record.identifier ?? record.issue ?? record.id ?? record.number ?? record.title);
  }
  const match = String(value).match(/#?(\d+)/);
  return match ? `#${match[1]}` : String(value);
}

function contentText(value: any): string {
  const content = value?.content ?? value?.text ?? value?.message ?? value?.output_text;
  if (Array.isArray(content)) {
    return content
      .map((part) => text(part?.text ?? part?.content ?? part))
      .filter(Boolean)
      .join('\n\n');
  }
  return previewText(content);
}

function summarizeArguments(value: unknown) {
  if (value == null || value === '') return 'No arguments.';
  if (typeof value === 'string') {
    try {
      return summarizeObject(JSON.parse(value));
    } catch {
      return previewText(value);
    }
  }
  return summarizeObject(value);
}

function summarizeObject(value: any) {
  if (value == null || typeof value !== 'object') return previewText(value);
  const entries = Object.entries(value).slice(0, 4);
  if (!entries.length) return '{}';
  return entries.map(([key, entry]) => `${key}: ${previewText(entry, 80)}`).join(' · ');
}

function usageSummary(usage: any) {
  const input = usage?.input_tokens ?? usage?.inputTokens ?? usage?.prompt_tokens;
  const output = usage?.output_tokens ?? usage?.outputTokens ?? usage?.completion_tokens;
  const total = usage?.total_tokens ?? usage?.totalTokens ?? (Number(input) || 0) + (Number(output) || 0);
  return [
    input != null ? `input ${input}` : null,
    output != null ? `output ${output}` : null,
    total ? `total ${total}` : null
  ].filter(Boolean).join(' · ') || previewText(usage);
}

function eventTimestamp(record: unknown) {
  if (!record || typeof record !== 'object') return undefined;
  const value = (record as any).timestamp ?? (record as any).time ?? (record as any).created_at ?? (record as any).payload?.timestamp ?? (record as any).payload?.started_at;
  return text(value) || undefined;
}

function latestUsage(events: TranscriptEvent[]) {
  return [...events].reverse().find((event) => event.kind === 'usage')?.body ?? null;
}

function diagnosticTitle(type: string, status: string) {
  if (/input|required/.test(`${type} ${status}`)) return 'Input required';
  if (/cancel|interrupt/.test(`${type} ${status}`)) return 'Session cancelled';
  if (/error|failed/.test(`${type} ${status}`)) return 'Session error';
  return 'Diagnostic event';
}

function ageLabel(ageMs: number | null) {
  if (ageMs == null) return 'unknown';
  const seconds = Math.round(ageMs / 1000);
  if (seconds < 90) return `${seconds}s ago`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 90) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  return `${hours}h ago`;
}

function numberOrNull(value: unknown) {
  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

function previewText(value: unknown, max = 900): string {
  const raw = typeof value === 'string' ? value : JSON.stringify(value ?? '', null, 2);
  const compact = raw.trim();
  return compact.length > max ? `${compact.slice(0, max)}...` : compact;
}

function text(value: unknown) {
  return String(value ?? '').trim();
}
