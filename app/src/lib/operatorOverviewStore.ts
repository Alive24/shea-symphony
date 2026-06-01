import { get } from 'svelte/store';
import { writable } from 'svelte/store';

import { loadOverview, loadReadSurface } from './operatorReads.ts';
import { mergeReadSurface } from './operatorReadModel.ts';
import { buildViewModel } from './operatorViewModel.ts';
import { refreshStatusStore } from './uiState.ts';

const slowSurfaces = ['autopilot', 'doctor', 'review', 'skills', 'sessions', 'local'];

export const operatorOverviewStore = writable({
  view: buildViewModel(null),
  loading: true,
  fullLoading: false,
  backgroundRefreshing: false,
  slowReadsRemaining: 0,
  liveError: ''
});

let readGeneration = 0;
let initialized = false;

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
    const overview = await loadOverview(force, 'fast');
    operatorOverviewStore.update((state) => ({
      ...state,
      view: buildViewModel(preserveLocalStatus(overview, state.view.raw)),
      loading: false
    }));
    if (!includeSlowReads) return;
    backgroundReadsStarted = true;
    startOperatorBackgroundReads(force, source, publishStatus);
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
      backgroundRefreshing: backgroundReadsStarted ? state.backgroundRefreshing : false
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
  const generation = ++readGeneration;
  const artifactSurfaces = ['sessions', 'local'];

  if (publishStatus) {
    refreshStatusStore.set({
      running: true,
      remaining: artifactSurfaces.length,
      startedAt: new Date().toISOString(),
      finishedAt: null,
      source,
      detail: 'Refreshing local artifacts'
    });
  }

  operatorOverviewStore.update((state) => ({
    ...state,
    liveError: '',
    slowReadsRemaining: artifactSurfaces.length
  }));

  for (const name of artifactSurfaces) {
    loadReadSurface(name, true)
      .then((surface) => {
        if (generation !== readGeneration) return;
        operatorOverviewStore.update((state) => ({
          ...state,
          view: buildViewModel(mergeReadSurface(state.view.raw, surface))
        }));
      })
      .catch((error) => {
        if (generation !== readGeneration) return;
        const message = error instanceof Error ? error.message : String(error ?? 'unknown error');
        operatorOverviewStore.update((state) => ({ ...state, liveError: message }));
      })
      .finally(() => {
        if (generation !== readGeneration) return;
        const nextRemaining = Math.max(0, get(operatorOverviewStore).slowReadsRemaining - 1);
        operatorOverviewStore.update((state) => ({
          ...state,
          slowReadsRemaining: nextRemaining
        }));
        if (publishStatus) {
          refreshStatusStore.update((status) => ({
            ...status,
            running: nextRemaining > 0,
            remaining: nextRemaining,
            finishedAt: nextRemaining === 0 ? new Date().toISOString() : status.finishedAt,
            detail: nextRemaining === 0 ? 'Local artifacts refreshed' : `Loading ${nextRemaining} local surface${nextRemaining === 1 ? '' : 's'}`
          }));
        }
      });
  }
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
      local: previousOverview?.commands?.local ?? nextOverview.commands?.local
    }
  };
}

function startOperatorBackgroundReads(force = false, source = 'manual', publishStatus = true) {
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

  for (const name of slowSurfaces) {
    loadReadSurface(name, force)
      .then((surface) => {
        if (generation !== readGeneration) return;
        operatorOverviewStore.update((state) => ({
          ...state,
          view: buildViewModel(mergeReadSurface(state.view.raw, surface))
        }));
      })
      .catch((error) => {
        if (generation !== readGeneration) return;
        const message = error instanceof Error ? error.message : String(error ?? 'unknown error');
        operatorOverviewStore.update((state) => ({
          ...state,
          liveError: message
        }));
      })
      .finally(() => {
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
      });
  }
}
