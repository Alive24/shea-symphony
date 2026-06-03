<script lang="ts">
  import { onMount } from 'svelte';
  import './app.css';
  import DraftSurface from './DraftSurface.svelte';
  import AppShell from './lib/AppShell.svelte';
  import {
    initializeOperatorOverview,
    operatorOverviewStore,
    requestOperatorDoctorRefresh,
    requestOperatorLocalArtifactsRefresh,
    requestOperatorOverviewRefresh
  } from './lib/operatorOverviewStore.ts';
  import {
    localArtifactRefreshEventDetail,
    shouldRequestLaneOverviewLocalRefresh
  } from './lib/localArtifactRefresh.ts';
  import { REFRESH_REQUEST_EVENT } from './lib/uiState.ts';
  import OperatorDesk from './OperatorDesk.svelte';

  let currentPath = '/';
  let lastLaneOverviewLocalRefreshMs = 0;
  $: operatorView = $operatorOverviewStore.view;

  function normalizePath(pathname: string) {
    const path = pathname || '/';
    return path === '/index.html' ? '/' : path;
  }

  function setRouteFromLocation() {
    currentPath = normalizePath(window.location.pathname);
  }

  function refreshRouteFromLocation() {
    setRouteFromLocation();
    requestLaneOverviewLocalRefresh('lane-overview-route');
  }

  function navigate(event: CustomEvent<{ href: string }>) {
    const href = event.detail?.href ?? '/';
    if (href === currentPath) return;
    window.history.pushState({}, '', href);
    currentPath = href;
    requestLaneOverviewLocalRefresh('lane-overview-route');
  }

  function requestLaneOverviewLocalRefresh(source = 'lane-overview-route') {
    const nowMs = Date.now();
    if (!shouldRequestLaneOverviewLocalRefresh(currentPath, nowMs, lastLaneOverviewLocalRefreshMs)) return;
    lastLaneOverviewLocalRefreshMs = nowMs;
    window.dispatchEvent(new CustomEvent(REFRESH_REQUEST_EVENT, {
      detail: localArtifactRefreshEventDetail(source)
    }));
  }

  function scheduleRefresh(force = false, includeSlowReads = true, source = 'manual', publishStatus = true) {
    if (publishStatus) {
      window.requestAnimationFrame(() => {
        window.setTimeout(() => {
          requestOperatorOverviewRefresh(force, includeSlowReads, source, publishStatus);
        }, 0);
      });
    } else {
      requestOperatorOverviewRefresh(force, includeSlowReads, source, publishStatus);
    }
  }

  onMount(() => {
    setRouteFromLocation();
    initializeOperatorOverview();
    const refreshRequestListener = (event: Event) => {
      const detail = (event as CustomEvent).detail ?? {};
      if (detail.localOnly) {
        requestOperatorLocalArtifactsRefresh(detail.source ?? 'local-artifacts');
        return;
      }
      if (detail.doctorOnly) {
        requestOperatorDoctorRefresh(detail.source ?? 'doctor');
        return;
      }
      scheduleRefresh(detail.force ?? true, true, detail.source ?? 'manual');
    };
    const focusListener = () => requestLaneOverviewLocalRefresh('lane-overview-focus');
    window.addEventListener('popstate', refreshRouteFromLocation);
    window.addEventListener('shea-navigate', navigate as EventListener);
    window.addEventListener(REFRESH_REQUEST_EVENT, refreshRequestListener);
    window.addEventListener('focus', focusListener);
    requestLaneOverviewLocalRefresh('lane-overview-route');
    return () => {
      window.removeEventListener('popstate', refreshRouteFromLocation);
      window.removeEventListener('shea-navigate', navigate as EventListener);
      window.removeEventListener(REFRESH_REQUEST_EVENT, refreshRequestListener);
      window.removeEventListener('focus', focusListener);
    };
  });
</script>

<svelte:head>
  <title>Shea Symphony App</title>
  <meta
    name="description"
    content="A high-fidelity local foreground workflow cockpit for Shea Symphony."
  />
</svelte:head>

<AppShell {currentPath}>
  {#if currentPath === '/'}
    <OperatorDesk />
  {:else}
    <DraftSurface route={currentPath} view={operatorView} />
  {/if}
</AppShell>
