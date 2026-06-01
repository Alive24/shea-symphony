<script lang="ts">
  import { onMount } from 'svelte';
  import {
    HANDOFF_TARGETS,
    HANDOFF_TARGET_CHANGE_EVENT,
    OPEN_AUTOLOOP_LOGS_EVENT,
    REFRESH_REQUEST_EVENT,
    START_DRY_RUN_EVENT,
    START_WRITE_EVENT,
    STOP_AUTOLOOP_EVENT,
    autoloopControlStore,
    autoloopStateStore,
    cliLogStore,
    defaultHandoffTargetStore,
    getDataMode,
    getDefaultHandoffTarget,
    refreshStatusStore,
    recordCliLog,
    resetFixtureOverview,
    setDataMode,
    setDefaultHandoffTarget,
    updateCliLog
  } from './uiState.ts';
  import {
    appendAutoloopLine,
    defaultLoopState,
    getGitHubUser,
    getLoopState,
    isTauriRuntime,
    mergeLaneSnapshot,
    startAutoloop,
    stopAutoloop,
    subscribeAutoloopEvents,
    type AutoloopLine,
    type LoopStateSnapshot,
    type GitHubUserSnapshot
  } from './tauriAutoloop.ts';
  import BrandRefreshStatus from './shell/BrandRefreshStatus.svelte';
  import CliLogModal from './shell/CliLogModal.svelte';
  import DeveloperToolsPanel from './shell/DeveloperToolsPanel.svelte';
  import JsonLogView from './shell/JsonLogView.svelte';
  import SettingsModal from './shell/SettingsModal.svelte';

  type ThemeMode = 'daylight' | 'night';
  type DataMode = 'live' | 'fixture';
  type HandoffTarget = 'codex-app' | 'claude-code' | 'gemini-cli';
  type RefreshInterval = 'manual' | '10000' | '30000' | '60000';
  type AutoloopLaneTarget = 'main' | 'review' | 'merge' | 'autoloop';

  export let currentPath = '/';

  const navItems = [
    { href: '/', label: 'Operator Desk' },
    { href: '/lanes', label: 'Lane Views' },
    { href: '/doctor', label: 'Doctor' },
    { href: '/intelligence', label: 'Intelligence' }
  ];

  let theme: ThemeMode = 'daylight';
  let dataMode: DataMode = 'live';
  let handoffTarget: HandoffTarget = 'codex-app';
  let refreshInterval: RefreshInterval = 'manual';
  let refreshTimer: number | undefined;
  let logsOpen = false;
  let jsonLogsOpen = false;
  let expandedJsonLogRows = new Set<string>();
  let autoloopBusy = false;
  let tauriAvailable = false;
  let tauriError = '';
  let autoloopState: LoopStateSnapshot = defaultLoopState();
  let settingsOpen = false;
  let developerToolsOpen = true;
  let developerToolsCollapsed = false;
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
  $: autoloopLogLines = autoloopState?.recentLines ?? [];
  $: autoloopStdoutLines = latestAutoloopStdout(autoloopState, autoloopLogLines);
  $: latestAutoloopLine = autoloopStdoutLines.slice(-1)[0]?.line ?? (autoloopState.running ? 'Loop is running' : 'No recent autoloop result');
  $: autoloopControlStore.set({
    tauriAvailable,
    busy: autoloopBusy,
    running: autoloopState.running,
    mode: autoloopState.mode,
    workflowPath: autoloopState.workflowPath,
    latestLine: latestAutoloopLine
  });
  $: autoloopStateStore.set(autoloopState);
  $: refreshRunning = $refreshStatusStore.running;
  $: refreshLabel = refreshRunning ? `Refreshing${$refreshStatusStore.remaining ? ` (${$refreshStatusStore.remaining})` : ''}` : 'Refresh';
  $: githubUserLabel = githubUser.available && githubUser.login ? `@${githubUser.login}` : 'gh unavailable';
  $: githubUserDetail = githubUser.available
    ? githubUser.email || 'GitHub CLI authenticated'
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
    requestRefresh('data-source');
  }

  function resetFixture() {
    dataMode = 'fixture';
    setDataMode('fixture');
    resetFixtureOverview();
    requestRefresh('fixture-reset');
  }

  function updateHandoffTarget(event: Event) {
    updateHandoffTargetValue((event.currentTarget as HTMLSelectElement).value as HandoffTarget);
  }

  function updateHandoffTargetValue(target: HandoffTarget) {
    handoffTarget = target;
    setDefaultHandoffTarget(handoffTarget);
    window.dispatchEvent(new CustomEvent(HANDOFF_TARGET_CHANGE_EVENT, { detail: { target: handoffTarget } }));
  }

  function requestRefresh(source = 'manual') {
    window.dispatchEvent(new CustomEvent(REFRESH_REQUEST_EVENT, { detail: { source, force: true } }));
  }

  function startDryRunFromTools() {
    window.dispatchEvent(new CustomEvent(START_DRY_RUN_EVENT));
  }

  function startDryRunWithMaxIterations(maxIterations: number, lane: AutoloopLaneTarget = 'autoloop') {
    window.dispatchEvent(new CustomEvent(START_DRY_RUN_EVENT, { detail: { maxIterations, lane } }));
  }

  function startDryRunForLane(lane: AutoloopLaneTarget) {
    window.dispatchEvent(new CustomEvent(START_DRY_RUN_EVENT, { detail: { lane } }));
  }

  function startWriteFromNav() {
    window.dispatchEvent(new CustomEvent(START_WRITE_EVENT));
  }

  function stopAutoloopFromNav() {
    window.dispatchEvent(new CustomEvent(STOP_AUTOLOOP_EVENT));
  }

  function openAutoloopLogsFromNav() {
    window.dispatchEvent(new CustomEvent(OPEN_AUTOLOOP_LOGS_EVENT));
  }

  function setDeveloperToolsOpen(open: boolean) {
    developerToolsOpen = open;
    if (open) setDeveloperToolsCollapsed(false);
    localStorage.setItem('shea-developer-tools-open', open ? 'true' : 'false');
  }

  function setDeveloperToolsCollapsed(collapsed: boolean) {
    developerToolsCollapsed = collapsed;
    localStorage.setItem('shea-developer-tools-collapsed', collapsed ? 'true' : 'false');
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

  function latestAutoloopStdout(state: LoopStateSnapshot, lines: AutoloopLine[]) {
    const startedAt = Number(state.startedAtMs);
    const lowerBound = Number.isFinite(startedAt) ? startedAt - 1000 : null;
    return lines.filter((entry) => entry.stream === 'stdout' && (lowerBound == null || entry.atMs >= lowerBound));
  }

  function formatAutoloopTime(value: unknown) {
    const time = Number(value);
    if (!Number.isFinite(time)) return '--:--:--';
    return new Date(time).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
  }

  function formatDuration(value: number | null | undefined) {
    if (value == null || !Number.isFinite(value)) return null;
    const duration = Math.max(0, Math.round(value));
    return duration >= 1000 ? `${(duration / 1000).toFixed(duration >= 10_000 ? 0 : 1)}s` : `${duration}ms`;
  }

  function toggleJsonLogRow(id: string) {
    const nextRows = new Set(expandedJsonLogRows);
    if (nextRows.has(id)) {
      nextRows.delete(id);
    } else {
      nextRows.add(id);
    }
    expandedJsonLogRows = nextRows;
  }

  async function refreshAutoloopState() {
    try {
      tauriAvailable = isTauriRuntime();
      autoloopState = await getLoopState();
    } catch (error) {
      tauriError = error.message;
    }
  }

  function laneConcurrency(lane: AutoloopLaneTarget) {
    if (lane === 'autoloop') return {};
    return {
      mainMaxConcurrent: lane === 'main' ? 1 : 0,
      reviewMaxConcurrent: lane === 'review' ? 1 : 0,
      mergeMaxConcurrent: lane === 'merge' ? 1 : 0
    };
  }

  async function startAutoloopMode(write: boolean, maxIterations?: number, lane: AutoloopLaneTarget = 'autoloop') {
    if (!tauriAvailable || autoloopBusy || autoloopState.running) return;
    autoloopBusy = true;
    tauriError = '';
    const startedAt = performance.now();
    const modeLabel = write ? 'write' : 'dry-run';
    const continuous = maxIterations == null;
    const loopArgs = continuous
      ? ['autopilot', 'loop', 'workflows/shea-symphony.md', '--continuous', write ? '--write' : '--dry-run']
      : ['autopilot', 'loop', 'workflows/shea-symphony.md', '--max-iterations', String(maxIterations), write ? '--write' : '--dry-run'];
    const laneOptions = laneConcurrency(lane);
    if (lane !== 'autoloop') {
      loopArgs.push('--main-max-concurrent', String(laneOptions.mainMaxConcurrent));
      loopArgs.push('--review-max-concurrent', String(laneOptions.reviewMaxConcurrent));
      loopArgs.push('--merge-max-concurrent', String(laneOptions.mergeMaxConcurrent));
    }
    const logId = recordCliLog({
      surface: 'autoloop',
      phase: 'start',
      status: 'running',
      detail: `Starting ${lane === 'autoloop' ? 'autoloop' : lane} ${modeLabel} ${continuous ? 'continuous' : `${maxIterations} iteration`} loop.`,
      args: loopArgs,
      raw: { args: loopArgs, lane, write, maxIterations: maxIterations ?? null, continuous, ...laneOptions }
    });
    try {
      autoloopState = await startAutoloop({
        workflowPath: 'workflows/shea-symphony.md',
        maxIterations,
        continuous,
        write,
        ...laneOptions
      });
      updateCliLog(logId, {
        surface: 'autoloop',
        phase: 'finish',
        status: 'ok',
        detail: autoloopState.pid ? `Autoloop started with pid ${autoloopState.pid}.` : 'Autoloop start command returned.',
        raw: autoloopState,
        durationMs: Math.round(performance.now() - startedAt)
      });
    } catch (error) {
      tauriError = error.message;
      updateCliLog(logId, {
        surface: 'autoloop',
        phase: 'error',
        status: 'failed',
        detail: error.message,
        durationMs: Math.round(performance.now() - startedAt)
      });
    } finally {
      autoloopBusy = false;
    }
  }

  async function stopRunningAutoloop() {
    if (!tauriAvailable || autoloopBusy || !autoloopState.running) return;
    autoloopBusy = true;
    tauriError = '';
    const startedAt = performance.now();
    const logId = recordCliLog({
      surface: 'autoloop',
      phase: 'stop',
      status: 'running',
      detail: 'Stopping autopilot loop.',
      raw: { action: 'stop', pid: autoloopState.pid ?? null }
    });
    try {
      autoloopState = await stopAutoloop();
      updateCliLog(logId, {
        surface: 'autoloop',
        phase: 'finish',
        status: 'ok',
        detail: 'Autoloop stop signal sent.',
        raw: autoloopState,
        durationMs: Math.round(performance.now() - startedAt)
      });
    } catch (error) {
      tauriError = error.message;
      updateCliLog(logId, {
        surface: 'autoloop',
        phase: 'error',
        status: 'failed',
        detail: error.message,
        durationMs: Math.round(performance.now() - startedAt)
      });
    } finally {
      autoloopBusy = false;
    }
  }

  function isActivePath(href: string) {
    if (href === '/') return currentPath === '/';
    if (href === '/doctor') return currentPath === '/doctor' || currentPath.startsWith('/doctor/') || currentPath === '/observability';
    if (href === '/intelligence') return currentPath === '/intelligence' || currentPath.startsWith('/intelligence/') || currentPath === '/reference';
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
    developerToolsCollapsed = localStorage.getItem('shea-developer-tools-collapsed') === 'true';
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
    refreshAutoloopState();
    let unlistenAutoloop: (() => void) | undefined;
    subscribeAutoloopEvents((event) => {
      if (event.type === 'line') {
        autoloopState = appendAutoloopLine(autoloopState, event.payload);
      } else if (event.type === 'lane') {
        autoloopState = mergeLaneSnapshot(autoloopState, event.payload);
      } else if (event.type === 'snapshot') {
        autoloopState = event.payload;
      } else if (event.type === 'started' || event.type === 'stopped' || event.type === 'error') {
        refreshAutoloopState();
      }
    }).then((unlisten) => {
      unlistenAutoloop = unlisten;
    });
    const startDryRunListener = (event: Event) => {
      const detail = (event as CustomEvent).detail ?? {};
      const rawMaxIterations = detail.maxIterations;
      const lane = ['main', 'review', 'merge', 'autoloop'].includes(detail.lane) ? detail.lane : 'autoloop';
      const maxIterations = rawMaxIterations == null ? undefined : Number(rawMaxIterations);
      startAutoloopMode(
        false,
        maxIterations == null || !Number.isFinite(maxIterations) ? undefined : Math.max(1, Math.round(maxIterations)),
        lane
      );
    };
    const startWriteListener = () => startAutoloopMode(true);
    const stopAutoloopListener = () => stopRunningAutoloop();
    const openAutoloopLogsListener = () => (logsOpen = true);
    window.addEventListener(START_DRY_RUN_EVENT, startDryRunListener);
    window.addEventListener(START_WRITE_EVENT, startWriteListener);
    window.addEventListener(STOP_AUTOLOOP_EVENT, stopAutoloopListener);
    window.addEventListener(OPEN_AUTOLOOP_LOGS_EVENT, openAutoloopLogsListener);
    configureRefreshTimer();
    defaultHandoffTargetStore.set(handoffTarget);
    return () => {
      if (refreshTimer) window.clearInterval(refreshTimer);
      window.removeEventListener(START_DRY_RUN_EVENT, startDryRunListener);
      window.removeEventListener(START_WRITE_EVENT, startWriteListener);
      window.removeEventListener(STOP_AUTOLOOP_EVENT, stopAutoloopListener);
      window.removeEventListener(OPEN_AUTOLOOP_LOGS_EVENT, openAutoloopLogsListener);
      unlistenAutoloop?.();
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
        class="runtime-action-button runtime-action-write"
        type="button"
        disabled={!$autoloopControlStore.tauriAvailable || $autoloopControlStore.busy || $autoloopControlStore.running}
        onclick={startWriteFromNav}
      >
        Start write
      </button>
      <button
        class="runtime-action-button"
        type="button"
        disabled={!$autoloopControlStore.tauriAvailable || $autoloopControlStore.busy || !$autoloopControlStore.running}
        onclick={stopAutoloopFromNav}
      >
        Stop
      </button>
      <button class="runtime-action-button" type="button" onclick={openAutoloopLogsFromNav}>Logs</button>
      <button
        class="theme-icon-button"
        type="button"
        aria-label="Toggle Day and Night theme"
        aria-pressed={theme === 'night'}
        onclick={toggleTheme}
      >
        {#if theme === 'daylight'}
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <circle cx="12" cy="12" r="4"></circle>
            <path d="M12 2v3M12 19v3M4.93 4.93l2.12 2.12M16.95 16.95l2.12 2.12M2 12h3M19 12h3M4.93 19.07l2.12-2.12M16.95 7.05l2.12-2.12"></path>
          </svg>
        {:else}
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="M20.2 14.4A7.6 7.6 0 0 1 9.6 3.8A8.7 8.7 0 1 0 20.2 14.4Z"></path>
          </svg>
        {/if}
      </button>
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
    {#if developerToolsOpen && !developerToolsCollapsed}
      <DeveloperToolsPanel
        width={developerToolsWidth}
        {dataMode}
        {refreshRunning}
        {latestLog}
        autoloopControl={$autoloopControlStore}
        onResizeStart={startDeveloperToolsResize}
        onHide={() => setDeveloperToolsCollapsed(true)}
        onToggleDataMode={toggleDataMode}
        onResetFixture={resetFixture}
        onOpenLogs={() => (jsonLogsOpen = true)}
        onStartDryRun={startDryRunFromTools}
        onStartDryRunWithMaxIterations={startDryRunWithMaxIterations}
        onStartDryRunForLane={startDryRunForLane}
      />
    {:else if developerToolsOpen}
      <aside class="developer-tools-collapsed" aria-label="Developer Tools collapsed">
        <button
          type="button"
          aria-label="Expand Developer Tools"
          onclick={() => setDeveloperToolsCollapsed(false)}
        >
          <span>Dev</span>
          <strong>Tools</strong>
        </button>
      </aside>
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
    {developerToolsOpen}
    onClose={() => (settingsOpen = false)}
    onHandoffTargetChange={updateHandoffTarget}
    onHandoffTargetSelect={updateHandoffTargetValue}
    onRefresh={() => requestRefresh('manual')}
    onRefreshIntervalChange={updateRefreshInterval}
    onDeveloperToolsVisibilityChange={updateDeveloperToolsVisibility}
  />
{/if}

{#if logsOpen}
  <CliLogModal onClose={() => (logsOpen = false)} />
{/if}

{#if jsonLogsOpen}
  <div class="modal-backdrop">
    <button class="modal-scrim" type="button" aria-label="Close CLI JSON log" onclick={() => (jsonLogsOpen = false)}></button>
    <div class="cli-log-modal autoloop-log-modal" role="dialog" aria-modal="true" aria-labelledby="autoloop-log-title">
      <header>
        <div>
          <p class="eyebrow">Developer Tools</p>
          <h2 id="autoloop-log-title">CLI JSON Log</h2>
          <span>{autoloopState.mode} · {autoloopState.workflowPath}</span>
        </div>
        <button class="btn btn-ghost" type="button" onclick={() => (jsonLogsOpen = false)}>Close</button>
      </header>

      {#if autoloopStdoutLines.length}
        <div class="autoloop-stdout-list" aria-label="CLI structured logs">
          {#each $cliLogStore as entry}
            <div class="autoloop-stdout-line cli-json-log-row">
              <time>{formatAutoloopTime(Date.parse(entry.at))}</time>
              <div>
                <button
                  class="cli-json-log-meta"
                  type="button"
                  aria-expanded={expandedJsonLogRows.has(`log-${entry.id}`)}
                  onclick={() => toggleJsonLogRow(`log-${entry.id}`)}
                >
                  <span class="json-tree-arrow" aria-hidden="true">›</span>
                  <strong>{entry.surface}</strong>
                  <span>{entry.phase}</span>
                  <span>{entry.status}</span>
                  {#if formatDuration(entry.durationMs)}
                    <span>{formatDuration(entry.durationMs)}</span>
                  {/if}
                </button>
                {#if expandedJsonLogRows.has(`log-${entry.id}`)}
                  <JsonLogView
                    value={entry.raw ?? { detail: entry.detail, args: entry.args, status: entry.status }}
                    fallbackLabel="CLI structured log"
                  />
                {/if}
                {#if entry.args?.length}
                  <code class="cli-json-command">{entry.args.join(' ')}</code>
                {/if}
              </div>
            </div>
          {/each}
          {#each autoloopStdoutLines as entry, index}
            <div class="autoloop-stdout-line">
              <time>{formatAutoloopTime(entry.atMs)}</time>
              <div>
                <button
                  class="cli-json-log-meta"
                  type="button"
                  aria-expanded={expandedJsonLogRows.has(`stdout-${entry.atMs}-${index}`)}
                  onclick={() => toggleJsonLogRow(`stdout-${entry.atMs}-${index}`)}
                >
                  <span class="json-tree-arrow" aria-hidden="true">›</span>
                  <strong>{entry.stream}</strong>
                  <span>{entry.event ? 'json' : 'text'}</span>
                </button>
                {#if expandedJsonLogRows.has(`stdout-${entry.atMs}-${index}`)}
                  <JsonLogView value={entry.event ?? entry.line} fallbackLabel="Autoloop stdout text" />
                {/if}
              </div>
            </div>
          {/each}
        </div>
      {:else if $cliLogStore.length}
        <div class="autoloop-stdout-list" aria-label="CLI structured logs">
          {#each $cliLogStore as entry}
            <div class="autoloop-stdout-line cli-json-log-row">
              <time>{formatAutoloopTime(Date.parse(entry.at))}</time>
              <div>
                <button
                  class="cli-json-log-meta"
                  type="button"
                  aria-expanded={expandedJsonLogRows.has(`log-${entry.id}`)}
                  onclick={() => toggleJsonLogRow(`log-${entry.id}`)}
                >
                  <span class="json-tree-arrow" aria-hidden="true">›</span>
                  <strong>{entry.surface}</strong>
                  <span>{entry.phase}</span>
                  <span>{entry.status}</span>
                  {#if formatDuration(entry.durationMs)}
                    <span>{formatDuration(entry.durationMs)}</span>
                  {/if}
                </button>
                {#if expandedJsonLogRows.has(`log-${entry.id}`)}
                  <JsonLogView
                    value={entry.raw ?? { detail: entry.detail, args: entry.args, status: entry.status }}
                    fallbackLabel="CLI structured log"
                  />
                {/if}
                {#if entry.args?.length}
                  <code class="cli-json-command">{entry.args.join(' ')}</code>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      {:else}
        <div class="cli-log-empty">
          <strong>No structured CLI output yet</strong>
          <p>{tauriError || 'Refresh or start a run to capture JSON payloads here.'}</p>
        </div>
      {/if}
    </div>
  </div>
{/if}
