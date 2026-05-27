<script lang="ts">
  import { onMount } from 'svelte';
  import {
    HANDOFF_TARGETS,
    HANDOFF_TARGET_CHANGE_EVENT,
    REFRESH_REQUEST_EVENT,
    cliLogStore,
    defaultHandoffTargetStore,
    getDataMode,
    getDefaultHandoffTarget,
    refreshStatusStore,
    resetFixtureOverview,
    setDataMode,
    setDefaultHandoffTarget
  } from './uiState.ts';

  type ThemeMode = 'daylight' | 'night';
  type DataMode = 'live' | 'fixture';
  type HandoffTarget = 'codex-app' | 'codex-cli' | 'github';
  type RefreshInterval = 'manual' | '10000' | '30000' | '60000';

  export let currentPath = '/';

  const navItems = [
    { href: '/', label: 'Operator Desk' },
    { href: '/lanes/main', label: 'Main Lane' },
    { href: '/lanes/review', label: 'Review Lane' },
    { href: '/lanes/merge', label: 'Merge Lane' },
    { href: '/observability', label: 'Observability' },
    { href: '/intelligence', label: 'Intelligence' },
    { href: '/reference', label: 'Reference' }
  ];

  let theme: ThemeMode = 'daylight';
  let dataMode: DataMode = 'live';
  let handoffTarget: HandoffTarget = 'codex-app';
  let refreshInterval: RefreshInterval = 'manual';
  let refreshTimer: number | undefined;
  let logsOpen = false;

  $: latestLog = $cliLogStore[0];
  $: refreshRunning = $refreshStatusStore.running;
  $: refreshLabel = refreshRunning ? `Refreshing${$refreshStatusStore.remaining ? ` (${$refreshStatusStore.remaining})` : ''}` : 'Refresh';

  function applyTheme(nextTheme: ThemeMode) {
    theme = nextTheme;
    document.documentElement.dataset.theme = nextTheme;
    localStorage.setItem('shea-theme', nextTheme);
  }

  function toggleTheme() {
    applyTheme(theme === 'daylight' ? 'night' : 'daylight');
  }

  function toggleDataMode() {
    dataMode = dataMode === 'fixture' ? 'live' : 'fixture';
    setDataMode(dataMode);
  }

  function resetFixture() {
    dataMode = 'fixture';
    setDataMode('fixture');
    resetFixtureOverview();
  }

  function updateHandoffTarget(event: Event) {
    handoffTarget = (event.currentTarget as HTMLSelectElement).value as HandoffTarget;
    setDefaultHandoffTarget(handoffTarget);
    window.dispatchEvent(new CustomEvent(HANDOFF_TARGET_CHANGE_EVENT, { detail: { target: handoffTarget } }));
  }

  function formatLogTime(value: string) {
    return new Date(value).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
  }

  function requestRefresh(source = 'manual') {
    window.dispatchEvent(new CustomEvent(REFRESH_REQUEST_EVENT, { detail: { source, force: true } }));
  }

  function updateRefreshInterval(event: Event) {
    refreshInterval = (event.currentTarget as HTMLSelectElement).value as RefreshInterval;
    localStorage.setItem('shea-refresh-interval', refreshInterval);
    configureRefreshTimer();
  }

  function configureRefreshTimer() {
    if (refreshTimer) {
      window.clearInterval(refreshTimer);
      refreshTimer = undefined;
    }
    if (refreshInterval === 'manual') return;
    refreshTimer = window.setInterval(() => {
      if (document.visibilityState !== 'visible' || $refreshStatusStore.running) return;
      requestRefresh('auto');
    }, Number(refreshInterval));
  }

  function isActivePath(href: string) {
    if (href === '/') return currentPath === '/';
    return currentPath === href || currentPath.startsWith(`${href}/`);
  }

  function navigate(event: MouseEvent, href: string) {
    event.preventDefault();
    window.dispatchEvent(new CustomEvent('shea-navigate', { detail: { href } }));
  }

  onMount(() => {
    const savedTheme = localStorage.getItem('shea-theme');
    applyTheme(savedTheme === 'night' ? 'night' : 'daylight');
    dataMode = getDataMode();
    handoffTarget = getDefaultHandoffTarget() as HandoffTarget;
    const savedRefreshInterval = localStorage.getItem('shea-refresh-interval');
    refreshInterval = ['manual', '10000', '30000', '60000'].includes(savedRefreshInterval ?? '')
      ? (savedRefreshInterval as RefreshInterval)
      : 'manual';
    configureRefreshTimer();
    defaultHandoffTargetStore.set(handoffTarget);
    return () => {
      if (refreshTimer) window.clearInterval(refreshTimer);
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

<div class="app-chrome">
  <header class="rail" aria-label="Primary navigation">
    <a class:loading={refreshRunning} class="brand-lockup" href="/" onclick={(event) => navigate(event, '/')}>
      <span class="brand-mark" aria-hidden="true">
        <span>SS</span>
        <span class="brand-loader"></span>
      </span>
      <span>
        <strong>Shea Symphony</strong>
        <small>App</small>
      </span>
    </a>

    <nav class="nav-list" aria-label="Current surface">
      {#each navItems as item}
        <a
          class:active={isActivePath(item.href)}
          href={item.href}
          onclick={(event) => navigate(event, item.href)}
        >
          {item.label}
        </a>
      {/each}
    </nav>

    <div class="topbar-cluster nav-actions" aria-label="Runtime state">
      <label class="handoff-default">
        <span>Handoff</span>
        <select value={handoffTarget} onchange={updateHandoffTarget} aria-label="Default handoff development environment">
          {#each HANDOFF_TARGETS as target}
            <option value={target.id}>{target.label}</option>
          {/each}
        </select>
      </label>
      <button
        class="mode-switch"
        type="button"
        aria-label="Toggle Live and Fixture data"
        aria-pressed={dataMode === 'fixture'}
        onclick={toggleDataMode}
      >
        <span>{dataMode === 'fixture' ? 'Fixture' : 'Live'}</span>
      </button>
      {#if dataMode === 'fixture'}
        <button class="mode-reset" type="button" onclick={resetFixture}>Reset</button>
      {/if}
      <button class="refresh-button" type="button" aria-busy={refreshRunning} onclick={() => requestRefresh('manual')}>
        {refreshLabel}
      </button>
      <label class="refresh-interval">
        <span>Auto</span>
        <select value={refreshInterval} onchange={updateRefreshInterval} aria-label="Auto refresh interval">
          <option value="manual">Manual</option>
          <option value="10000">10s</option>
          <option value="30000">30s</option>
          <option value="60000">1m</option>
        </select>
      </label>
      <button
        class="cli-log-toggle"
        type="button"
        aria-label="Open CLI command log"
        aria-pressed={logsOpen}
        onclick={() => (logsOpen = true)}
      >
        <span>CLI Logs</span>
        {#if refreshRunning}
          <small>{$refreshStatusStore.remaining || '...'}</small>
        {:else if latestLog}
          <small>{latestLog.status}</small>
        {/if}
      </button>
      <button
        class="theme-toggle"
        type="button"
        aria-label="Toggle Day and Night theme"
        aria-pressed={theme === 'night'}
        onclick={toggleTheme}
      >
        <span>{theme === 'daylight' ? 'Day' : 'Night'}</span>
      </button>
    </div>
  </header>

  <section class="workspace">
    <main class="screen-shell">
      <slot />
    </main>
  </section>
</div>

{#if logsOpen}
  <div class="modal-backdrop">
    <button class="modal-scrim" type="button" aria-label="Close CLI command log" onclick={() => (logsOpen = false)}></button>
    <div class="cli-log-modal" role="dialog" aria-modal="true" aria-labelledby="cli-log-title">
      <header>
        <div>
          <p class="eyebrow">Runtime</p>
          <h2 id="cli-log-title">CLI Command Log</h2>
        </div>
        <button class="btn btn-ghost" type="button" onclick={() => (logsOpen = false)}>Close</button>
      </header>

      {#if $cliLogStore.length}
        <div class="cli-log-list">
          {#each $cliLogStore as entry}
            <article class="cli-log-row {entry.status}">
              <div>
                <span>{formatLogTime(entry.at)}</span>
                <strong>{entry.surface}</strong>
                <em>{entry.phase}</em>
              </div>
              <p>{entry.detail || entry.status}</p>
              <footer>
                <span>{entry.status}</span>
                {#if entry.durationMs != null}
                  <span>{Math.round(entry.durationMs)}ms</span>
                {/if}
                {#if entry.args?.length}
                  <code>{entry.args.join(' ')}</code>
                {/if}
              </footer>
            </article>
          {/each}
        </div>
      {:else}
        <div class="cli-log-empty">
          <strong>No CLI command activity yet</strong>
          <p>Read surfaces and autoloop controls will appear here when they start.</p>
        </div>
      {/if}
    </div>
  </div>
{/if}
