export function mergeReadSurface(overview: any, surface: any) {
  if (!overview || !surface?.name) return overview;
  const next = {
    ...overview,
    generatedAt: surface.generatedAt ?? overview.generatedAt,
    scope: 'incremental',
    commands: {
      ...(overview.commands ?? {}),
      [surface.name]: surface.command
    }
  };

  if (surface.name === 'sessions') {
    next.sessionsText = surface.text ?? '';
  } else if (surface.name === 'local') {
    next.localStatus = mergeLocalStatus(overview.localStatus, surface.parsed);
  } else {
    next[surface.name] = surface.parsed ?? null;
  }

  next.healthy = Object.values(next.commands).some((result: any) => result?.ok);
  return next;
}

function mergeLocalStatus(previous: any, next: any) {
  if (!next) return previous ?? null;
  if (!previous) return next;
  return {
    ...previous,
    ...next,
    issueWorktrees: Object.prototype.hasOwnProperty.call(next, 'issueWorktrees')
      ? next.issueWorktrees
      : previous.issueWorktrees,
    completedIssueWorktrees: Object.prototype.hasOwnProperty.call(next, 'completedIssueWorktrees')
      ? next.completedIssueWorktrees
      : previous.completedIssueWorktrees,
    issueLifecycle: Object.prototype.hasOwnProperty.call(next, 'issueLifecycle')
      ? next.issueLifecycle
      : previous.issueLifecycle
  };
}
