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
    next.localStatus = surface.parsed ?? null;
  } else {
    next[surface.name] = surface.parsed ?? null;
  }

  next.healthy = Object.values(next.commands).some((result: any) => result?.ok);
  return next;
}
