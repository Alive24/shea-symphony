<script lang="ts">
  import { onMount } from 'svelte';
  import {
    HANDOFF_TARGETS,
    HANDOFF_TARGET_CHANGE_EVENT,
    REFRESH_REQUEST_EVENT,
    START_DRY_RUN_EVENT,
    autoloopControlStore,
    cliLogStore,
    defaultHandoffTargetStore,
    getDataMode,
    getDefaultHandoffTarget,
    refreshStatusStore,
    resetFixtureOverview,
    setDataMode,
    setDefaultHandoffTarget
  } from './uiState.ts';
  import { getGitHubUser, type GitHubUserSnapshot } from './tauriAutoloop.ts';
  import BrandRefreshStatus from './shell/BrandRefreshStatus.svelte';
  import CliLogModal from './shell/CliLogModal.svelte';
  import DeveloperToolsPanel from './shell/DeveloperToolsPanel.svelte';
  import SettingsModal from './shell/SettingsModal.svelte';

  type ThemeMode = 'daylight' | 'night';
  type DataMode = 'live' | 'fixture';
  type HandoffTarget = 'codex-app' | 'codex-cli' | 'github';
  type RefreshInterval = 'manual' | '10000' | '30000' | '60000';

  export let currentPath = '/';

  const navItems = [
    { href: '/', label: 'Operator Desk' },
    { href: '/lanes', label: 'Lane Views' },
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
  let settingsOpen = false;
  let developerToolsOpen = true;
  let developerToolsWidth = 340;
  let resizingDeveloperTools = false;
  let githubUser: GitHubUserSnapshot = {
    available: false,
    login: '',
    name: '',
    email: '',
    avatarUrl: '',
    error: 'Loading GitHub identity'
  };

  $: latestLog = $cliLogStore[0];
  $: refreshRunning = $refreshStatusStore.running;
  $: refreshLabel = refreshRunning ? `Refreshing${$refreshStatusStore.remaining ? ` (${$refreshStatusStore.remaining})` : ''}` : 'Refresh';
  $: githubUserLabel = githubUser.available && githubUser.login ? `@${githubUser.login}` : 'gh unavailable';
  $: githubUserDetail = githubUser.available
    ? githubUser.name || githubUser.email || 'GitHub CLI authenticated'
    : githubUser.error || 'GitHub CLI unavailable';

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

  function requestRefresh(source = 'manual') {
    window.dispatchEvent(new CustomEvent(REFRESH_REQUEST_EVENT, { detail: { source, force: true } }));
  }

  function startDryRunFromTools() {
    window.dispatchEvent(new CustomEvent(START_DRY_RUN_EVENT));
  }

  function setDeveloperToolsOpen(open: boolean) {
    developerToolsOpen = open;
    localStorage.setItem('shea-developer-tools-open', open ? 'true' : 'false');
  }

  function updateDeveloperToolsVisibility(event: Event) {
    setDeveloperToolsOpen((event.currentTarget as HTMLInputElement).checked);
  }

  function startDeveloperToolsResize(event: PointerEvent) {
    resizingDeveloperTools = true;
    const startX = event.clientX;
    const startWidth = developerToolsWidth;
    const resize = (moveEvent: PointerEvent) => {
      const nextWidth = startWidth - (moveEvent.clientX - startX);
      developerToolsWidth = Math.min(520, Math.max(280, nextWidth));
      localStorage.setItem('shea-developer-tools-width', String(developerToolsWidth));
    };
    const stop = () => {
      resizingDeveloperTools = false;
      window.removeEventListener('pointermove', resize);
      window.removeEventListener('pointerup', stop);
    };
    window.addEventListener('pointermove', resize);
    window.addEventListener('pointerup', stop);
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
    developerToolsOpen = localStorage.getItem('shea-developer-tools-open') !== 'false';
    const savedDeveloperToolsWidth = Number(localStorage.getItem('shea-developer-tools-width'));
    if (Number.isFinite(savedDeveloperToolsWidth) && savedDeveloperToolsWidth > 0) {
      developerToolsWidth = Math.min(520, Math.max(280, savedDeveloperToolsWidth));
    }
    getGitHubUser().then((user) => {
      githubUser = user;
    }).catch((error) => {
      githubUser = {
        available: false,
        login: '',
        name: '',
        email: '',
        avatarUrl: '',
        error: error.message
      };
    });
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
        <BrandRefreshStatus
          running={refreshRunning}
          remaining={$refreshStatusStore.remaining}
          finishedAt={$refreshStatusStore.finishedAt}
        />
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
      <button
        class="menu-button"
        type="button"
        aria-label={`Settings ${githubUserLabel}`}
        aria-haspopup="dialog"
        aria-expanded={settingsOpen}
        onclick={() => (settingsOpen = true)}
      >
        {#if githubUser.avatarUrl}
          <img src={githubUser.avatarUrl} alt="" />
        {:else}
          <span class="menu-avatar" aria-hidden="true">gh</span>
        {/if}
        <span class="menu-user-id">{githubUserLabel}</span>
      </button>
    </div>
  </header>

  <section class:developer-tools-resizing={resizingDeveloperTools} class="workspace">
    <main class="screen-shell">
      <slot />
    </main>
    {#if developerToolsOpen}
      <DeveloperToolsPanel
        width={developerToolsWidth}
        {dataMode}
        {refreshRunning}
        {latestLog}
        autoloopControl={$autoloopControlStore}
        onResizeStart={startDeveloperToolsResize}
        onHide={() => setDeveloperToolsOpen(false)}
        onToggleDataMode={toggleDataMode}
        onResetFixture={resetFixture}
        onOpenLogs={() => (logsOpen = true)}
        onStartDryRun={startDryRunFromTools}
      />
    {/if}
  </section>
</div>

{#if settingsOpen}
  <SettingsModal
    {githubUser}
    {githubUserLabel}
    {githubUserDetail}
    handoffTargets={HANDOFF_TARGETS}
    {handoffTarget}
    {refreshInterval}
    {refreshRunning}
    {refreshLabel}
    {theme}
    {developerToolsOpen}
    onClose={() => (settingsOpen = false)}
    onHandoffTargetChange={updateHandoffTarget}
    onRefresh={() => requestRefresh('manual')}
    onRefreshIntervalChange={updateRefreshInterval}
    onToggleTheme={toggleTheme}
    onDeveloperToolsVisibilityChange={updateDeveloperToolsVisibility}
  />
{/if}

{#if logsOpen}
  <CliLogModal onClose={() => (logsOpen = false)} />
{/if}
