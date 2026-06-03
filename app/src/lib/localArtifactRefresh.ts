export const LOCAL_ARTIFACT_READ_SURFACES = ['sessions', 'status'] as const;
export const LANE_OVERVIEW_LOCAL_REFRESH_INTERVAL_MS = 15_000;

export type LocalArtifactRefreshStatus = {
  running: boolean;
  remaining: number;
  startedAt: string | null;
  lastRefreshedAt: string | null;
  error: string;
  source: string;
};

export function localArtifactRefreshEventDetail(source = 'lane-overview-local') {
  return {
    source,
    force: true,
    localOnly: true
  };
}

export function shouldRequestLaneOverviewLocalRefresh(
  route: string,
  nowMs: number,
  lastRequestedAtMs: number,
  minIntervalMs = LANE_OVERVIEW_LOCAL_REFRESH_INTERVAL_MS
) {
  return route === '/lanes' && nowMs - lastRequestedAtMs >= minIntervalMs;
}

export function localRefreshStatusLabel(status: LocalArtifactRefreshStatus | null | undefined, formatTime: (value: unknown) => string) {
  if (status?.running) {
    const remaining = Number(status.remaining ?? 0);
    return `Refreshing local artifacts${remaining > 0 ? ` · ${remaining} remaining` : ''}`;
  }
  if (status?.error) return `Local refresh failed · ${status.error}`;
  if (status?.lastRefreshedAt) return `Local artifacts refreshed ${formatTime(status.lastRefreshedAt)}`;
  return 'Local artifacts not refreshed this session';
}
