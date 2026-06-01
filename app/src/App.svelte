<script lang="ts">
  import { onMount } from 'svelte';
  import './app.css';
  import DraftSurface from './DraftSurface.svelte';
  import AppShell from './lib/AppShell.svelte';
  import {
    initializeOperatorOverview,
    operatorOverviewStore,
    requestOperatorLocalArtifactsRefresh,
    requestOperatorOverviewRefresh
  } from './lib/operatorOverviewStore.ts';
  import { REFRESH_REQUEST_EVENT } from './lib/uiState.ts';
  import OperatorDesk from './OperatorDesk.svelte';

  let currentPath = '/';
  $: operatorView = $operatorOverviewStore.view;

  function normalizePath(pathname: string) {
    const path = pathname || '/';
    return path === '/index.html' ? '/' : path;
  }

  function setRouteFromLocation() {
    currentPath = normalizePath(window.location.pathname);
  }

  function navigate(event: CustomEvent<{ href: string }>) {
    const href = event.detail?.href ?? '/';
    if (href === currentPath) return;
    window.history.pushState({}, '', href);
    currentPath = href;
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
      scheduleRefresh(detail.force ?? true, true, detail.source ?? 'manual');
    };
    window.addEventListener('popstate', setRouteFromLocation);
    window.addEventListener('shea-navigate', navigate as EventListener);
    window.addEventListener(REFRESH_REQUEST_EVENT, refreshRequestListener);
    return () => {
      window.removeEventListener('popstate', setRouteFromLocation);
      window.removeEventListener('shea-navigate', navigate as EventListener);
      window.removeEventListener(REFRESH_REQUEST_EVENT, refreshRequestListener);
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
