import { get } from 'svelte/store';
import { writable } from 'svelte/store';

import { loadOverview, loadReadSurface } from './operatorReads.ts';
import { mergeReadSurface } from './operatorReadModel.ts';
import { buildViewModel } from './operatorViewModel.ts';
import {
  LOCAL_ARTIFACT_READ_SURFACES,
  type LocalArtifactRefreshStatus
} from './localArtifactRefresh.ts';
import { refreshStatusStore } from './uiState.ts';

const slowSurfaces = ['githubQueue', 'skills', 'sessions', 'status'];
const projectReadSurfaces = new Set(['autopilot', 'doctor', 'review', 'githubQueue']);

export const defaultBackgroundReadSurfaces = [...slowSurfaces];
export const projectCooldownReadSurfaces = [...projectReadSurfaces];

const idleLocalArtifactsRefresh: LocalArtifactRefreshStatus = {
  running: false,
  remaining: 0,
  startedAt: null,
  lastRefreshedAt: null,
  error: '',
  source: 'idle'
};

export const operatorOverviewStore = writable({
  view: buildViewModel(null),
  loading: true,
  fullLoading: false,
  backgroundRefreshing: false,
  slowReadsRemaining: 0,
  liveError: '',
  localArtifactsRefresh: idleLocalArtifactsRefresh,
  projectReadCooldown: null
});

let readGeneration = 0;
let localArtifactsGeneration = 0;
let initialized = false;
let backgroundReadsInFlight = false;
let projectReadCooldownUntilMs = 0;

export function initializeOperatorOverview() {
  if (initialized) return;
  initialized = true;
  requestOperatorOverviewRefresh(false, true, 'initial', false);
}

export async function requestOperatorOverviewRefresh(
  force = false,
  includeSlowReads = true,
  source = 'manual',
  publishStatus = true
) {
  const current = get(operatorOverviewStore);
  const hasRenderableState = current.view?.dataSource?.mode !== 'offline';
  let backgroundReadsStarted = false;

  operatorOverviewStore.update((state) => ({
    ...state,
    backgroundRefreshing: hasRenderableState,
    loading: !hasRenderableState,
    fullLoading: includeSlowReads,
    slowReadsRemaining: 0,
    liveError: ''
  }));

  if (publishStatus) {
    refreshStatusStore.set({
      running: true,
      remaining: includeSlowReads ? slowSurfaces.length : 1,
      startedAt: new Date().toISOString(),
      finishedAt: null,
      source,
      detail: 'Requesting overview'
    });
  }

  try {
    const overview = applyStableProjectQueueIfPaused(await loadOverview(force, 'fast'), current.view.raw);
    operatorOverviewStore.update((state) => ({
      ...state,
      view: buildViewModel(preserveFastOverviewState(overview, state.view.raw)),
      projectReadCooldown: projectCooldownFromOverview(overview) ?? state.projectReadCooldown,
      loading: false
    }));
    if (!includeSlowReads) return;
    backgroundReadsStarted = startOperatorBackgroundReads(force, source, publishStatus);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error ?? 'unknown error');
    operatorOverviewStore.update((state) => ({
      ...state,
      liveError: message,
      view: hasRenderableState ? state.view : buildViewModel(null)
    }));
    if (publishStatus) {
      refreshStatusStore.set({
        running: false,
        remaining: 0,
        startedAt: null,
        finishedAt: new Date().toISOString(),
        source,
        detail: message
      });
    }
  } finally {
    operatorOverviewStore.update((state) => ({
      ...state,
      loading: false,
      backgroundRefreshing: backgroundReadsStarted || backgroundReadsInFlight ? state.backgroundRefreshing : false
    }));
    if (!includeSlowReads && publishStatus) {
      refreshStatusStore.set({
        running: false,
        remaining: 0,
        startedAt: null,
        finishedAt: new Date().toISOString(),
        source,
        detail: 'Overview refreshed'
      });
    }
  }
}

export function requestOperatorLocalArtifactsRefresh(source = 'local-artifacts', publishStatus = true) {
  const generation = ++localArtifactsGeneration;
  const artifactSurfaces = [...LOCAL_ARTIFACT_READ_SURFACES];
  const startedAt = new Date().toISOString();

  if (publishStatus) {
    refreshStatusStore.set({
      running: true,
      remaining: artifactSurfaces.length,
      startedAt,
      finishedAt: null,
      source,
      detail: 'Refreshing local artifacts'
    });
  }

  operatorOverviewStore.update((state) => ({
    ...state,
    liveError: '',
    localArtifactsRefresh: {
      running: true,
      remaining: artifactSurfaces.length,
      startedAt,
      lastRefreshedAt: state.localArtifactsRefresh?.lastRefreshedAt ?? null,
      error: '',
      source
    }
  }));

  for (const name of artifactSurfaces) {
    loadReadSurface(name, true, false)
      .then((surface) => {
        if (generation !== localArtifactsGeneration) return;
        operatorOverviewStore.update((state) => ({
          ...state,
          view: buildViewModel(mergeReadSurface(state.view.raw, surface)),
          projectReadCooldown: projectCooldownFromSurface(surface) ?? state.projectReadCooldown
        }));
      })
      .catch((error) => {
        if (generation !== localArtifactsGeneration) return;
        const message = error instanceof Error ? error.message : String(error ?? 'unknown error');
        operatorOverviewStore.update((state) => ({
          ...state,
          localArtifactsRefresh: {
            ...(state.localArtifactsRefresh ?? {}),
            running: false,
            remaining: state.localArtifactsRefresh?.remaining ?? 0,
            startedAt: state.localArtifactsRefresh?.startedAt ?? null,
            lastRefreshedAt: state.localArtifactsRefresh?.lastRefreshedAt ?? null,
            error: message,
            source
          }
        }));
      })
      .finally(() => {
        if (generation !== localArtifactsGeneration) return;
        const currentLocalStatus = get(operatorOverviewStore).localArtifactsRefresh;
        const nextRemaining = Math.max(0, Number(currentLocalStatus?.remaining ?? 0) - 1);
        const finishedAt = nextRemaining === 0 ? new Date().toISOString() : null;
        operatorOverviewStore.update((state) => ({
          ...state,
          localArtifactsRefresh: {
            ...(state.localArtifactsRefresh ?? {}),
            running: nextRemaining > 0,
            remaining: nextRemaining,
            startedAt: state.localArtifactsRefresh?.startedAt ?? null,
            lastRefreshedAt: finishedAt && !state.localArtifactsRefresh?.error
              ? finishedAt
              : state.localArtifactsRefresh?.lastRefreshedAt ?? null,
            error: state.localArtifactsRefresh?.error ?? '',
            source
          }
        }));
        if (publishStatus) {
          refreshStatusStore.update((status) => ({
            ...status,
            running: nextRemaining > 0,
            remaining: nextRemaining,
            finishedAt: finishedAt ?? status.finishedAt,
            detail: nextRemaining === 0 ? 'Local artifacts refreshed' : `Loading ${nextRemaining} local surface${nextRemaining === 1 ? '' : 's'}`
          }));
        }
      });
  }
}

export function requestOperatorDoctorRefresh(source = 'doctor', publishStatus = true) {
  const generation = ++readGeneration;

  if (publishStatus) {
    refreshStatusStore.set({
      running: true,
      remaining: 1,
      startedAt: new Date().toISOString(),
      finishedAt: null,
      source,
      detail: 'Refreshing doctor'
    });
  }

  operatorOverviewStore.update((state) => ({
    ...state,
    liveError: '',
    fullLoading: true,
    backgroundRefreshing: true,
    slowReadsRemaining: 1
  }));

  if (projectReadCooldownActive()) {
    finishSkippedBackgroundSurface(generation, source, publishStatus);
    return;
  }

  loadReadSurface('doctor', true)
    .then((surface) => {
      if (generation !== readGeneration) return;
      const cooldown = projectCooldownFromSurface(surface);
      if (cooldown) projectReadCooldownUntilMs = Math.max(projectReadCooldownUntilMs, cooldown.untilMs);
      operatorOverviewStore.update((state) => ({
        ...state,
        view: buildViewModel(mergeReadSurface(state.view.raw, surface)),
        projectReadCooldown: cooldown ?? state.projectReadCooldown
      }));
    })
    .catch((error) => {
      if (generation !== readGeneration) return;
      const message = error instanceof Error ? error.message : String(error ?? 'unknown error');
      operatorOverviewStore.update((state) => ({ ...state, liveError: message }));
    })
    .finally(() => {
      finishLoadedBackgroundSurface(generation, publishStatus);
    });
}

function preserveFastOverviewState(nextOverview: any, previousOverview: any) {
  return preserveDeferredProjectStatus(
    preserveDoctorStatus(preserveLocalStatus(nextOverview, previousOverview), previousOverview),
    previousOverview
  );
}

function preserveLocalStatus(nextOverview: any, previousOverview: any) {
  if (!nextOverview || nextOverview.localStatus) return nextOverview;
  const previousLocalStatus = previousOverview?.localStatus;
  if (!previousLocalStatus) return nextOverview;
  return {
    ...nextOverview,
    localStatus: previousLocalStatus,
    commands: {
      ...(nextOverview.commands ?? {}),
      status: previousOverview?.commands?.status ?? nextOverview.commands?.status
    }
  };
}

function preserveDoctorStatus(nextOverview: any, previousOverview: any) {
  const nextDoctorCommand = nextOverview?.commands?.doctor;
  const previousDoctorCommand = previousOverview?.commands?.doctor;
  if (!nextOverview || !nextDoctorCommand?.pending || !previousDoctorCommand || previousDoctorCommand.pending) {
    return nextOverview;
  }
  return {
    ...nextOverview,
    doctor: previousOverview?.doctor ?? nextOverview.doctor,
    commands: {
      ...(nextOverview.commands ?? {}),
      doctor: previousDoctorCommand
    }
  };
}

function startOperatorBackgroundReads(force = false, source = 'manual', publishStatus = true) {
  if (backgroundReadsInFlight) {
    operatorOverviewStore.update((state) => ({
      ...state,
      backgroundRefreshing: true,
      fullLoading: true
    }));
    if (publishStatus) {
      refreshStatusStore.set({
        running: false,
        remaining: get(operatorOverviewStore).slowReadsRemaining,
        startedAt: null,
        finishedAt: new Date().toISOString(),
        source,
        detail: 'Background refresh already in progress'
      });
    }
    return false;
  }

  backgroundReadsInFlight = true;
  const generation = ++readGeneration;
  operatorOverviewStore.update((state) => ({
    ...state,
    fullLoading: true,
    slowReadsRemaining: slowSurfaces.length
  }));

  if (publishStatus) {
    refreshStatusStore.update((status) => ({
      ...status,
      running: true,
      remaining: slowSurfaces.length,
      source,
      detail: 'Loading CLI read surfaces'
    }));
  }

  void runBackgroundReadsSequentially(generation, force, source, publishStatus)
    .finally(() => {
      backgroundReadsInFlight = false;
    });
  return true;
}

async function runBackgroundReadsSequentially(generation: number, force: boolean, source: string, publishStatus: boolean) {
  for (const name of slowSurfaces) {
    if (generation !== readGeneration) return;
    if (projectReadSurfaces.has(name) && projectReadCooldownActive()) {
      finishSkippedBackgroundSurface(generation, source, publishStatus);
      continue;
    }
    try {
      const surface = await loadReadSurface(name, force);
      if (generation !== readGeneration) return;
      const cooldown = projectCooldownFromSurface(surface);
      if (cooldown) projectReadCooldownUntilMs = Math.max(projectReadCooldownUntilMs, cooldown.untilMs);
      operatorOverviewStore.update((state) => ({
        ...state,
        view: buildViewModel(
          applyStableProjectQueueIfPaused(
            mergeReadSurface(state.view.raw, surface),
            state.view.raw
          )
        ),
        projectReadCooldown: cooldown ?? state.projectReadCooldown
      }));
    } catch (error) {
      if (generation !== readGeneration) return;
      const message = error instanceof Error ? error.message : String(error ?? 'unknown error');
      operatorOverviewStore.update((state) => ({
        ...state,
        liveError: message
      }));
    } finally {
      finishLoadedBackgroundSurface(generation, publishStatus);
    }
  }
}

function preserveDeferredProjectStatus(nextOverview: any, previousOverview: any) {
  let merged = nextOverview;
  for (const name of ['autopilot', 'review']) {
    merged = preserveDeferredSurfaceStatus(merged, previousOverview, name);
  }
  return merged;
}

function preserveDeferredSurfaceStatus(nextOverview: any, previousOverview: any, name: string) {
  const nextCommand = nextOverview?.commands?.[name];
  const previousCommand = previousOverview?.commands?.[name];
  if (!nextOverview || !nextCommand?.pending || !previousCommand || previousCommand.pending) {
    return nextOverview;
  }
  return {
    ...nextOverview,
    [name]: previousOverview?.[name] ?? nextOverview[name],
    commands: {
      ...(nextOverview.commands ?? {}),
      [name]: previousCommand
    }
  };
}

function finishLoadedBackgroundSurface(generation: number, publishStatus: boolean) {
  if (generation !== readGeneration) return;
  const nextRemaining = Math.max(0, get(operatorOverviewStore).slowReadsRemaining - 1);
  operatorOverviewStore.update((state) => ({
    ...state,
    slowReadsRemaining: nextRemaining,
    fullLoading: nextRemaining > 0,
    backgroundRefreshing: nextRemaining > 0
  }));
  if (publishStatus) {
    refreshStatusStore.update((status) => ({
      ...status,
      running: nextRemaining > 0,
      remaining: nextRemaining,
      finishedAt: nextRemaining === 0 ? new Date().toISOString() : status.finishedAt,
      detail: nextRemaining === 0
        ? 'Refresh complete'
        : `Loading ${nextRemaining} CLI surface${nextRemaining === 1 ? '' : 's'}`
    }));
  }
}

function finishSkippedBackgroundSurface(generation: number, source: string, publishStatus: boolean) {
  if (generation !== readGeneration) return;
  const nextRemaining = Math.max(0, get(operatorOverviewStore).slowReadsRemaining - 1);
  operatorOverviewStore.update((state) => ({
    ...state,
    slowReadsRemaining: nextRemaining,
    fullLoading: nextRemaining > 0,
    backgroundRefreshing: nextRemaining > 0
  }));
  if (publishStatus) {
    refreshStatusStore.update((status) => ({
      ...status,
      running: nextRemaining > 0,
      remaining: nextRemaining,
      source,
      finishedAt: nextRemaining === 0 ? new Date().toISOString() : status.finishedAt,
      detail: nextRemaining === 0
        ? 'Refresh complete'
        : `Project read paused; loading ${nextRemaining} CLI surface${nextRemaining === 1 ? '' : 's'}`
    }));
  }
}

function projectReadCooldownActive() {
  return projectReadCooldownUntilMs > Date.now();
}

function projectCooldownFromOverview(overview: any) {
  return projectCooldownFromCommand(overview?.commands?.githubQueue)
    ?? projectCooldownFromParsed(overview?.githubQueue)
    ?? projectCooldownFromCommand(overview?.commands?.autopilot)
    ?? projectCooldownFromCommand(overview?.commands?.doctor)
    ?? projectCooldownFromCommand(overview?.commands?.review);
}

function projectCooldownFromSurface(surface: any) {
  return projectCooldownFromCommand(surface?.command) ?? projectCooldownFromParsed(surface?.parsed);
}

function projectCooldownFromCommand(command: any) {
  if (!command?.projectReadPaused && command?.signal !== 'project-rate-limit-cooldown') return null;
  return normalizeProjectCooldown(command.rateLimitResetAtMs, command.stderr);
}

function projectCooldownFromParsed(parsed: any) {
  if (!parsed?.projectReadPaused && parsed?.failureKind !== 'rate_limit') return null;
  return normalizeProjectCooldown(parsed.rateLimitResetAtMs, parsed.reason);
}

function normalizeProjectCooldown(resetAtMs: any, reason: any) {
  const untilMs = Number(resetAtMs);
  const cooldown = {
    untilMs: Number.isFinite(untilMs) && untilMs > 0 ? untilMs : Date.now() + 10 * 60 * 1000,
    reason: String(reason ?? 'GitHub Project GraphQL read is paused after rate limit.')
  };
  projectReadCooldownUntilMs = Math.max(projectReadCooldownUntilMs, cooldown.untilMs);
  return cooldown;
}

function applyStableProjectQueueIfPaused(nextOverview: any, previousOverview: any) {
  if (!nextOverview) return nextOverview;
  const cooldown = projectCooldownFromOverview(nextOverview);
  if (!cooldown) return nextOverview;
  const previousQueue = previousOverview?.githubQueue;
  if (!previousQueue?.issues?.length) return nextOverview;
  return {
    ...nextOverview,
    githubQueue: {
      ...previousQueue,
      projectReadPaused: true,
      rateLimitResetAtMs: cooldown.untilMs,
      reason: cooldown.reason,
      source: `${previousQueue.source ?? 'project state'} · last stable during Project read cooldown`
    },
    commands: {
      ...(nextOverview.commands ?? {}),
      githubQueue: nextOverview.commands?.githubQueue ?? previousOverview?.commands?.githubQueue
    }
  };
}
