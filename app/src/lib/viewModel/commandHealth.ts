import {
  commandDetail,
  commandImpact,
  commandRecommendation,
  exitLabel,
  labelForCommand
} from './commandHelpers.ts';

type LooseRecord = Record<string, any>;

export function buildCommandHealth(commands: LooseRecord) {
  return ['githubQueue', 'autopilot', 'doctor', 'review', 'skills', 'sessions', 'status'].map((name) => {
    const result = commands[name];
    const status = result ? (result.ok ? 'Passed' : result.pending ? 'Pending' : 'Failed') : 'Not checked';
    const detail = result ? commandDetail(result) : 'No result captured from this overview command.';
    return {
      id: name,
      label: labelForCommand(name),
      status,
      tone: result ? (result.ok ? 'success' : result.pending ? 'warn' : 'danger') : 'warn',
      duration: result?.durationMs == null ? 'n/a' : `${Math.round(result.durationMs / 1000)}s`,
      detail,
      exit: result ? exitLabel(result) : 'n/a',
      impact: commandImpact(name, result),
      recommendation: commandRecommendation(name, result),
      args: result?.args?.join(' ') ?? 'not run'
    };
  });
}

export function buildReadPathMap(commandHealth: any[]) {
  const byId = new Map((commandHealth ?? []).map((command) => [command.id, command]));
  return [
    {
      id: 'tauri-bridge',
      label: 'Tauri Bridge',
      role: 'Desktop command and event bridge',
      status: 'Available',
      tone: 'success',
      detail: 'The desktop shell invokes allowlisted Shea Symphony CLI reads and streams autoloop events.'
    },
    readPathNode(byId.get('autopilot'), {
      id: 'tracker',
      label: 'Tracker Posture',
      role: 'Autoloop plan',
      detail: 'Feeds lane counts, selected issues, and parked queues.'
    }),
    readPathNode(byId.get('githubQueue'), {
      id: 'github-queue',
      label: 'GitHub Queue',
      role: 'Open issue Project scan',
      detail: 'Feeds first-screen lane pulse and operator queue counts.'
    }),
    readPathNode(byId.get('doctor'), {
      id: 'readiness',
      label: 'Readiness',
      role: 'Doctor',
      detail: 'Feeds blockers, repair recommendations, and install health.'
    }),
    readPathNode(byId.get('review'), {
      id: 'review',
      label: 'Review Evidence',
      role: 'Review status',
      detail: 'Feeds Agent Review and Human Review freshness.'
    }),
    readPathNode(byId.get('skills'), {
      id: 'skills',
      label: 'Skill Coverage',
      role: 'Skills status',
      detail: 'Feeds installed Skill visibility and setup gaps.'
    }),
    readPathNode(byId.get('sessions'), {
      id: 'sessions',
      label: 'Foreground Sessions',
      role: 'Session list',
      detail: 'Feeds active agent session presence.'
    }),
    readPathNode(byId.get('status'), {
      id: 'status',
      label: 'Status',
      role: 'Runtime/session status',
      detail: 'Feeds branch, dirty count, worktree count, build, and binary readiness.'
    })
  ];
}

function readPathNode(command: any, fallback: any) {
  const timedOut = command?.exit?.startsWith('timeout');
  return {
    ...fallback,
    status: command?.status ?? 'Not checked',
    tone: command ? (timedOut || command.status === 'Pending' ? 'warn' : command.tone) : 'warn',
    detail: command ? command.impact : fallback.detail,
    signal: command ? command.detail : 'No read captured.',
    exit: command?.exit ?? 'n/a'
  };
}
