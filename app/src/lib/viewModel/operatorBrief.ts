export function buildOperatorBrief({ attentionTasks, laneSummaries, evidenceColumns, dataSource }) {
  const sortedTasks = [...(attentionTasks ?? [])].sort(
    (left, right) => severityRank(right.tone) - severityRank(left.tone)
  );
  const focus = sortedTasks[0] ?? null;
  const skillCounts = new Map([
    ['Manual Main', { label: 'Manual Main', count: 0, tone: 'neutral' }],
    ['Manual Review', { label: 'Manual Review', count: 0, tone: 'neutral' }],
    ['Human Review', { label: 'Human Review', count: 0, tone: 'neutral' }],
    ['Doctor', { label: 'Doctor', count: 0, tone: 'neutral' }]
  ]);

  for (const task of attentionTasks ?? []) {
    const skill = skillForTask(task);
    const row = skillCounts.get(skill) ?? { label: skill, count: 0, tone: 'neutral' };
    row.count += 1;
    row.tone = toneForCount(row.count, task.tone);
    skillCounts.set(skill, row);
  }

  const lanes = (laneSummaries ?? []).map((lane) => ({
    name: lane.name,
    pressure: Number(lane.active ?? 0) + Number(lane.blocked ?? 0) + Number(lane.retrying ?? 0),
    tone:
      lane.sourceTone === 'danger'
        ? 'danger'
        : Number(lane.blocked ?? 0)
          ? 'danger'
          : lane.sourceTone === 'warn' || Number(lane.retrying ?? 0)
            ? 'warn'
            : 'success',
    provenance: lane.provenance ?? 'fallback',
    sourceLabel: lane.sourceLabel ?? 'Unknown source'
  }));
  const laneMax = Math.max(1, ...lanes.map((lane) => lane.pressure));
  const laneSources = new Set(lanes.map((lane) => lane.provenance));
  const sourceNote = laneSources.has('fallback')
    ? 'Lane counts include fallback posture'
    : laneSources.has('partial')
      ? 'Lane counts are partial'
      : laneSources.has('fixture')
        ? 'Fixture lane posture'
        : 'Lane counts from live reads';
  const evidence = (evidenceColumns ?? []).map((column) => ({
    lane: column.lane,
    count: column.events?.length ?? 0
  }));

  return {
    focus,
    trust: dataSource?.trust ?? 'Confirm in chat Skills before routing',
    sourceNote,
    skills: [...skillCounts.values()],
    lanes,
    laneMax,
    evidence
  };
}

function skillForTask(task) {
  const state = normalizeTaskState(task.type ?? task.urgency);
  const text = `${task.title ?? ''} ${task.reason ?? ''} ${task.recommended ?? ''} ${task.evidence ?? ''}`;
  if (/Human Review|human decision|approve to Merging/i.test(text)) return 'Human Review';
  if (/Agent Review|review evidence|finding/i.test(text)) return 'Manual Review';
  if (state === 'Human Review') return 'Human Review';
  if (state === 'Agent Review') return 'Manual Review';
  if (state === 'Need Human Input' || state === 'Diagnostics') return 'Doctor';
  return 'Manual Main';
}

function normalizeTaskState(value) {
  return String(value || 'Unknown')
    .replace(/[-_]/g, ' ')
    .replace(/\b\w/g, (letter) => letter.toUpperCase());
}

function severityRank(tone) {
  return { danger: 3, warn: 2, success: 1, neutral: 0 }[tone] ?? 0;
}

function toneForCount(count, sourceTone) {
  if (sourceTone === 'danger') return 'danger';
  if (sourceTone === 'warn' || count > 0) return 'warn';
  return 'neutral';
}
