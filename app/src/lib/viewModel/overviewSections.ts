import {
  commandActionForDiagnostic,
  commandDetail,
  commandEvidence,
  labelForCommand
} from './commandHelpers.ts';
import { countLaneStatus, eventRowsFromAutopilot, laneSourceFor } from './laneActivity.ts';
import { firstLine, timeLabel, titleCase } from './text.ts';

export function buildCommandFailures(commands: Record<string, any>) {
  return Object.entries(commands)
    .filter(([, result]: [string, any]) => result && !result.ok && !result.pending)
    .map(([name, result]) => ({
      id: name,
      title: `${labelForCommand(name)} unavailable`,
      type: 'Diagnostics',
      reason: commandDetail(result),
      action: 'Inspect',
      recommended: `Open diagnostics for ${labelForCommand(name)} and repair the underlying local dependency.`,
      evidence: commandEvidence(result),
      urgency: 'Needs repair',
      tone: 'warn',
      decisions: [
        {
          label: 'Retry',
          result: `Rerun ${labelForCommand(name)} from the web UI.`,
          writes: 'Read-only command.',
          commandAction: commandActionForDiagnostic(name)
        }
      ]
    }));
}

export function buildReadinessItems(commands: Record<string, any>) {
  return [
    readinessFromCommand('Autoloop plan', commands.autopilot),
    readinessFromCommand('Doctor', commands.doctor),
    readinessFromCommand('Review status', commands.review),
    readinessFromCommand('Skills', commands.skills)
  ];
}

export function buildLaneSummaries({
  autopilot,
  githubQueue,
  commands,
  overview,
  fallbackLaneSummaries
}) {
  return ['main', 'review', 'merge'].map((lane) => {
    const lanePlan = (autopilot?.lanes ?? []).find((item) => item.lane === lane);
    const githubLaneCount = Number(githubQueue?.laneCounts?.[lane] ?? 0);
    const fallback = fallbackLaneSummaries.find((item) => item.name.toLowerCase() === lane);
    const source = laneSourceFor(lanePlan, commands.autopilot, overview, commands.githubQueue, githubQueue);
    return {
      name: titleCase(lane),
      href: '/lanes',
      active: lanePlan
        ? lanePlan.selected_issue
          ? 1
          : 0
        : githubQueue?.laneCounts
          ? githubLaneCount
          : source.countsReliable === false || source.provenance === 'live'
            ? 0
            : fallback?.active ?? 0,
      retrying: countLaneStatus(autopilot, lane, 'retrying'),
      blocked: lanePlan?.status === 'blocked' ? 1 : 0,
      latest: lanePlan
        ? `${lanePlan.action ?? 'No action'}: ${lanePlan.reason ?? lanePlan.status}`
        : githubQueue?.laneCounts
          ? `${githubLaneCount} open Project item${githubLaneCount === 1 ? '' : 's'} in lane states.`
          : source.provenance === 'live'
            ? 'Live autoloop read returned no selected issue for this lane.'
            : fallback?.latest ?? 'No live lane data.',
      posture:
        lanePlan?.status ??
        (githubLaneCount > 0 ? 'queued' : source.provenance === 'live' ? 'idle' : fallback?.posture ?? 'unknown'),
      ...source
    };
  });
}

export function buildRecentEvents(autopilot: any, commands: Record<string, any>, generatedAt: Date | null) {
  return [
    ...eventRowsFromAutopilot(autopilot),
    ...Object.entries(commands).map(([name, result]: [string, any]) => ({
      time: generatedAt ? timeLabel(generatedAt) : 'now',
      lane: 'System',
      title: `${labelForCommand(name)} ${result.ok ? 'passed' : result.pending ? 'pending' : 'failed'}`,
      detail: result.ok
        ? `Completed in ${Math.round(result.durationMs / 1000)}s.`
        : firstLine(result.stderr || result.stdoutPreview || 'Command failed.')
    }))
  ].slice(0, 8);
}

function readinessFromCommand(label: string, result: any) {
  if (!result) return { label, status: 'Unknown', tone: 'warn' };
  if (result.ok) return { label, status: 'Ready', tone: 'success' };
  if (result.pending) return { label, status: 'Loading', tone: 'warn' };
  return { label, status: 'Blocked', tone: 'danger' };
}
