import { titleCase } from './text.ts';

export function normalizeIssueRef(value: any) {
  const match = String(value ?? '').match(/#?(\d+)/);
  return match ? `#${match[1]}` : null;
}

export function stateToLane(state: any) {
  if (!isLaneQueueState(state)) return 'Unknown';
  if (isHumanOperatorQueueState(state)) return 'Human';
  if (state === 'Agent Review') return 'Review';
  if (state === 'Merging') return 'Merge';
  return 'Main';
}

export function isHumanOperatorQueueState(state: any) {
  return ['Need to Clarify', 'Need Human Input', 'Human Review'].includes(normalizeStateName(state));
}

export function isLaneQueueState(state: any) {
  return [
    'Need to Clarify',
    'Todo',
    'In Progress',
    'Rework',
    'Agent Review',
    'Human Review',
    'Merging',
    'Need Human Input'
  ].includes(normalizeStateName(state));
}

export function normalizeStateName(value: any) {
  const normalized = titleCase(value || 'Unknown');
  const aliases: Record<string, string> = {
    Main: 'In Progress',
    Review: 'Agent Review',
    Merge: 'Merging',
    Parked: 'Need Human Input',
    Diagnostics: 'Need Human Input',
    'Need To Clarify': 'Need to Clarify'
  };
  return aliases[normalized] ?? normalized;
}

export function toneForState(state: any) {
  if (state === 'Need Human Input') return 'danger';
  if (state === 'Human Review' || state === 'Rework') return 'warn';
  if (['In Progress', 'Agent Review', 'Merging'].includes(state)) return 'success';
  return 'neutral';
}

export function issueRefFromValue(value: any): string | null {
  if (value == null || value === '' || value === 'none') return null;
  if (typeof value === 'number') return `#${value}`;
  if (typeof value === 'string') {
    const trimmed = value.trim();
    if (!trimmed || trimmed === 'none') return null;
    const match = trimmed.match(/#?(\d+)/);
    return match ? `#${match[1]}` : trimmed;
  }
  if (typeof value === 'object') {
    return issueRefFromValue(
      value.identifier ??
        value.issue ??
        value.id ??
        value.number ??
        value.url ??
        value.html_url ??
        value.title
    );
  }
  return String(value);
}
