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
  };
};

export type HeartbeatSummary = {
  state: 'running' | 'stale' | 'stopped' | 'unavailable';
  tone: TranscriptTone;
  label: string;
  lastHeartbeatMs: number | null;
  lastHeartbeatAge: string;
  latestLaneEvent: string;
};

const staleAfterMs = 2 * 60 * 1000;

export function classifyHeartbeat(
  loopState: any,
  laneKey: string,
  nowMs = Date.now()
): HeartbeatSummary {
  const lane = loopState?.lanes?.[laneKey];
  const lastHeartbeatMs = numberOrNull(lane?.updatedAtMs ?? latestLineAt(loopState));
  const ageMs = lastHeartbeatMs == null ? null : Math.max(0, nowMs - lastHeartbeatMs);
  const latestLaneEvent = usefulLaneEvent(loopState, laneKey) ?? lane?.latestLine ?? lane?.action ?? 'No lane event visible.';

  if (!loopState) {
    return heartbeat('unavailable', null, 'unavailable', 'Autoloop state is unavailable.', latestLaneEvent);
  }
  if (!loopState.running) {
    return heartbeat('stopped', lastHeartbeatMs, ageLabel(ageMs), 'Loop stopped', latestLaneEvent);
  }
  if (lastHeartbeatMs == null) {
    return heartbeat('unavailable', null, 'unavailable', 'Heartbeat unavailable', latestLaneEvent);
  }
  if (ageMs != null && ageMs > staleAfterMs) {
    return heartbeat('stale', lastHeartbeatMs, ageLabel(ageMs), 'Heartbeat stale', latestLaneEvent);
  }
  return heartbeat('running', lastHeartbeatMs, ageLabel(ageMs), 'Loop running', latestLaneEvent);
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
    if (mapped) events.push(mapped);
  });

  const summary = {
    userTurns: events.filter((event) => event.kind === 'user').length,
    assistantTurns: events.filter((event) => event.kind === 'assistant' || event.kind === 'final').length,
    toolCalls: events.filter((event) => event.kind === 'tool_call').length,
    diagnostics: events.filter((event) => event.kind === 'diagnostic').length,
    tokenUsage: latestUsage(events)
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
  const item = record?.item ?? record?.payload?.item ?? record?.message ?? record;
  const type = text(record?.type ?? item?.type ?? record?.event ?? record?.kind);
  const role = text(item?.role ?? record?.role);
  const status = text(item?.status ?? record?.status);
  const name = text(item?.name ?? item?.tool_name ?? record?.name ?? record?.tool_name);
  const usage = record?.usage ?? record?.token_usage ?? record?.response?.usage;

  if (usage && !role && !item?.content && !record?.content) {
    return event(index, 'usage', 'Token usage', usageSummary(usage), 'neutral', undefined, record);
  }

  if (role === 'user' || type === 'user_message') {
    return event(index, 'user', 'User', contentText(item || record), 'neutral', type || undefined, record);
  }
  if (role === 'assistant' || type === 'assistant_message' || type === 'message') {
    const body = contentText(item || record);
    const final = /final|answer/i.test(type) || record?.is_final === true;
    return event(index, final ? 'final' : 'assistant', final ? 'Final answer' : 'Assistant', body, final ? 'success' : 'neutral', status || undefined, record);
  }
  if (/final|response\.completed/.test(type)) {
    const body = contentText(item || record) || text(record?.response?.output_text);
    if (body) return event(index, 'final', 'Final answer', body, 'success', status || undefined, record);
  }
  if (type.includes('function_call_output') || type.includes('tool_output') || item?.output) {
    return event(index, 'tool_output', name || 'Tool output', previewText(item?.output ?? record?.output ?? record?.result), status === 'failed' ? 'danger' : 'neutral', status || undefined, record);
  }
  if (type.includes('function_call') || type.includes('tool_call') || item?.arguments || item?.input) {
    const args = item?.arguments ?? item?.input ?? record?.arguments ?? record?.input;
    return event(index, 'tool_call', name || 'Tool call', summarizeArguments(args), 'neutral', status || 'started', record);
  }
  if (/error|cancel|interrupt|input|required|failed/.test(`${type} ${status}`)) {
    return event(index, 'diagnostic', diagnosticTitle(type, status), previewText(record?.error ?? record?.message ?? record), /error|failed/.test(`${type} ${status}`) ? 'danger' : 'warn', status || undefined, record);
  }
  if (usage) {
    return event(index, 'usage', 'Token usage', usageSummary(usage), 'neutral', undefined, record);
  }
  return null;
}

function event(index: number, kind: TranscriptEventKind, title: string, body: string, tone: TranscriptTone, detail?: string, raw?: unknown): TranscriptEvent {
  return {
    id: `${kind}-${index}`,
    kind,
    title,
    body: body || '(empty)',
    detail,
    tone,
    raw
  };
}

function heartbeat(state: HeartbeatSummary['state'], lastHeartbeatMs: number | null, lastHeartbeatAge: string, label: string, latestLaneEvent: string): HeartbeatSummary {
  const tone = state === 'running' ? 'success' : state === 'stale' ? 'warn' : state === 'stopped' ? 'neutral' : 'warn';
  return { state, tone, label, lastHeartbeatMs, lastHeartbeatAge, latestLaneEvent };
}

function usefulLaneEvent(loopState: any, laneKey: string) {
  const lines = [...(loopState?.recentLines ?? [])].reverse();
  for (const entry of lines) {
    const event = entry?.event;
    const payload = event?.payload ?? {};
    const raw = text(payload.raw ?? entry.line);
    const lane = text(payload.lane ?? payload.fields?.lane ?? event?.lane);
    if (lane && lane !== laneKey) continue;
    if (isNoisyLine(raw)) continue;
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
    || /reason=state is not active\b/.test(raw)
    || /reason=no_(merging|agent_review)_issue\b/.test(raw);
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
  const input = usage?.input_tokens ?? usage?.prompt_tokens;
  const output = usage?.output_tokens ?? usage?.completion_tokens;
  const total = usage?.total_tokens ?? (Number(input) || 0) + (Number(output) || 0);
  return [
    input != null ? `input ${input}` : null,
    output != null ? `output ${output}` : null,
    total ? `total ${total}` : null
  ].filter(Boolean).join(' · ') || previewText(usage);
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
