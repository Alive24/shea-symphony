<script lang="ts">
  import { onMount } from 'svelte';
  import './app.css';
  import DraftSurface from './DraftSurface.svelte';
  import AppShell from './lib/AppShell.svelte';
  import { buildFixtureOverview } from './lib/operatorFixtures.ts';
  import { buildViewModel } from './lib/operatorViewModel.ts';
  import OperatorDesk from './OperatorDesk.svelte';

  let currentPath = '/';
  const draftView = buildViewModel(buildFixtureOverview(false));

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

  onMount(() => {
    setRouteFromLocation();
    window.addEventListener('popstate', setRouteFromLocation);
    window.addEventListener('shea-navigate', navigate as EventListener);
    return () => {
      window.removeEventListener('popstate', setRouteFromLocation);
      window.removeEventListener('shea-navigate', navigate as EventListener);
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
    <DraftSurface route={currentPath} view={draftView} />
  {/if}
</AppShell>
