import { firstLine, titleCase } from './text.ts';

export function commandDetail(result: any) {
  if (result.ok) return firstLine(result.stdoutPreview || 'Command completed.');
  if (result.pending) return firstLine(result.stderr || 'Deferred to full overview.');
  if (result.timedOut) return `Timed out after ${Math.round((result.durationMs ?? 0) / 1000)}s.`;
  return firstLine(result.stderr || result.stdoutPreview || 'Command failed.');
}

export function commandEvidence(result: any) {
  if (result.timedOut) {
    return `The command exceeded the Web overview timeout and was stopped with ${result.signal ?? 'a termination signal'}.`;
  }
  return result.stderr || result.stdoutPreview || 'No command output was captured.';
}

export function exitLabel(result: any) {
  if (result.pending) return 'pending';
  if (result.timedOut) return `timeout / ${result.signal ?? 'terminated'}`;
  if (result.exitCode == null) return result.signal ?? 'n/a';
  return String(result.exitCode);
}

export function commandImpact(name: string, result: any) {
  if (!result) return 'This read surface has not been checked yet.';
  if (result.pending) return 'This slower read surface is loading in the full overview pass.';
  if (result.ok) {
    const impacts: Record<string, string> = {
      autopilot: 'Lane queue posture and selected work can be trusted.',
      doctor: 'Readiness blockers and repair recommendations are visible.',
      review: 'Agent Review and Human Review evidence can be inspected.',
      skills: 'Installed Shea Skill coverage is observable.',
      sessions: 'Foreground agent session presence is observable.',
      status: 'Runtime sessions, local checkout, binary, and worktree posture are observable.',
      githubQueue: 'Open issue Project status counts are available for the first-screen lane pulse.'
    };
    return impacts[name] ?? 'This read surface is available.';
  }
  const impacts: Record<string, string> = {
    autopilot: 'Lane counts may fall back to static posture and parked queues may be incomplete.',
    doctor: 'Readiness blockers may be hidden until Doctor returns.',
    review: 'Review freshness and Human Review evidence may be incomplete.',
    skills: 'Skill installation/readiness status may be hidden.',
    sessions: 'Active foreground sessions may be hidden.',
    status: 'Runtime and local status posture may be hidden.',
    githubQueue: 'First-screen lane pulse may be stale or rely on slower tracker reads.'
  };
  return impacts[name] ?? 'This read surface is degraded.';
}

export function commandRecommendation(name: string, result: any) {
  if (!result) return 'Refresh overview after the local server is available.';
  if (result.ok) return 'Use this signal for observation.';
  if (result.pending) return 'Use fast overview for immediate posture; wait for full overview before trusting this surface.';
  if (result.timedOut) return 'Treat as slow read surface; inspect Diagnostics before trusting related counts.';
  return `Inspect ${labelForCommand(name)} output and local dependencies.`;
}

export function labelForCommand(name: string) {
  const labels: Record<string, string> = {
    autopilot: 'Autopilot plan',
    doctor: 'Doctor',
    review: 'Review status',
    skills: 'Skills status',
    sessions: 'Session list',
    status: 'Status',
    githubQueue: 'GitHub queue'
  };
  return labels[name] ?? titleCase(name);
}

export function commandActionForDiagnostic(name: string) {
  const actions: Record<string, string> = {
    autopilot: 'autopilot-plan',
    doctor: 'doctor',
    review: 'review-status',
    skills: 'skills-status'
  };
  return actions[name] ?? 'autopilot-plan';
}
