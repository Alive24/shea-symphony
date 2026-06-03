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
    operatorRunLogLines,
    startAutoloop,
    stopAutoloop,
    subscribeAutoloopEvents,
    type AutoloopLine,
    type LaneSnapshot,
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
  type RunLogSummary = {
    eventName: string;
    title: string;
    detail: string;
    chips: string[];
    tone: 'info' | 'success' | 'warn' | 'error';
  };

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
  let runLogsOpen = false;
  let expandedRunLogRows = new Set<string>();
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
    latestLine: latestAutoloopLine,
    laneMaxSummary: laneMaxSummary(autoloopState?.lanes)
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
    return operatorRunLogLines(state, lines);
  }

  function laneMaxSummary(lanes: Record<string, LaneSnapshot> | undefined) {
    const parts = ['main', 'review', 'merge']
      .map((laneKey) => {
        const value = Number(lanes?.[laneKey]?.maxConcurrent);
        return Number.isFinite(value) ? `${laneKey} ${value}` : null;
      })
      .filter(Boolean);
    return parts.length ? `max · ${parts.join(' · ')}` : '';
  }

  function formatAutoloopTime(value: unknown) {
    const time = Number(value);
    if (!Number.isFinite(time)) return '--:--:--';
    return new Date(time).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
  }

  function toggleRunLogRow(id: string) {
    const nextRows = new Set(expandedRunLogRows);
    if (nextRows.has(id)) {
      nextRows.delete(id);
    } else {
      nextRows.add(id);
    }
    expandedRunLogRows = nextRows;
  }

  function runLogSummary(entry: AutoloopLine): RunLogSummary {
    const event = entry.event;
    if (!event) {
      return {
        eventName: entry.stream,
        title: entry.stream === 'stderr' ? 'stderr output' : 'stdout output',
        detail: compactRunLine(entry.line),
        chips: [entry.stream],
        tone: entry.stream === 'stderr' ? 'warn' : 'info'
      };
    }

    const eventName = stringField(event, 'event') ?? 'event';
    const payload = objectField(event, 'payload') ?? {};
    if (eventName === 'autopilot_signal') {
      const status = stringField(payload, 'status') ?? stringField(payload, 'kind') ?? 'event';
      const lane = stringField(payload, 'lane');
      const issue = stringField(payload, 'issue');
      const action = stringField(payload, 'action');
      const title = issue
        ? `${issue} ${status}`
        : stringField(payload, 'message') ?? status;
      return {
        eventName,
        title,
        detail: stringField(payload, 'message') ?? ([lane, action].filter(Boolean).join(' · ') || compactRunLine(entry.line)),
        chips: [lane, status, action, stringField(payload, 'visibility')].filter(Boolean),
        tone: runLogTone(status, 0)
      };
    }
    if (eventName === 'autopilot_loop_status') {
      const phase = stringField(payload, 'phase') ?? 'unknown';
      const counts = objectField(payload, 'counts') ?? {};
      const selected = selectedIssuesLabel(arrayField(payload, 'selected_issues'));
      const blockers = arrayField(payload, 'blocked_reasons')?.length ?? 0;
      return {
        eventName,
        title: `Loop ${phase}`,
        detail: stringField(payload, 'message') ?? 'Loop status updated.',
        chips: [
          ...concurrencyChips(objectField(payload, 'settings')),
          `running ${numberField(counts, 'running') ?? 0}`,
          `blocked ${numberField(counts, 'blocked') ?? blockers}`,
          selected
        ].filter(Boolean),
        tone: runLogTone(phase, blockers)
      };
    }
    if (eventName === 'autopilot_loop_lane') {
      const lane = stringField(payload, 'lane') ?? 'lane';
      const status = stringField(payload, 'status') ?? 'unknown';
      const action = stringField(payload, 'action') ?? 'event';
      const selected = selectedIssueLabel(objectField(payload, 'selected_issue')) ?? stringField(payload, 'selected') ?? 'none';
      const workUnit = booleanField(payload, 'work_unit_completed');
      const detail = workUnit && selected !== 'none'
        ? `handled ${selected}`
        : `${action} · selected ${selected}`;
      return {
        eventName,
        title: `${lane} ${status}`,
        detail,
        chips: [lane, status, action, workUnit ? 'issue handled' : '', maxConcurrentChip(payload)].filter(Boolean),
        tone: runLogTone(status, 0)
      };
    }
    if (eventName === 'autopilot_loop_iteration') {
      const iteration = numberField(payload, 'iteration');
      const mode = stringField(payload, 'mode') ?? autoloopState.mode;
      const order = arrayField(payload, 'order')?.map(String).join(' -> ');
      return {
        eventName,
        title: `Iteration ${iteration ?? '?'}`,
        detail: order ? `${mode} · ${order}` : `${mode} iteration started.`,
        chips: [mode, ...concurrencyChips(objectField(payload, 'settings'))],
        tone: 'info'
      };
    }
    if (eventName === 'autopilot_loop_result') {
      const cycle = numberField(payload, 'supervisor_cycle') ?? numberField(payload, 'iteration');
      const workUnits = numberField(payload, 'completed_work_units') ?? numberField(payload, 'work_units');
      const completedThisCycle = numberField(payload, 'work_units_completed_this_cycle') ?? 0;
      const limit = numberField(payload, 'work_unit_limit');
      const lanes = arrayField(payload, 'lanes') ?? [];
      const laneSummary = lanes
        .map((lane) => {
          const value = objectFromUnknown(lane);
          return `${stringField(value, 'lane') ?? 'lane'}:${stringField(value, 'status') ?? 'unknown'}`;
        })
        .join('  ');
      const hasError = lanes.some((lane) => stringField(objectFromUnknown(lane), 'status') === 'error');
      const handledLabel = limit != null
        ? `Issues handled ${workUnits ?? 0} / ${limit}`
        : completedThisCycle > 0
          ? `Issues handled +${completedThisCycle}`
          : 'Loop result';
      return {
        eventName,
        title: handledLabel,
        detail: laneSummary || `Supervisor cycle ${cycle ?? '?'} completed.`,
        chips: [
          ...concurrencyChips(objectField(payload, 'settings')),
          ...lanes.slice(0, 3).map((lane) => stringField(objectFromUnknown(lane), 'status') ?? 'unknown')
        ],
        tone: hasError ? 'error' : 'success'
      };
    }
    if (eventName === 'autopilot_loop_stopped') {
      const cycles = numberField(payload, 'supervisor_cycles') ?? numberField(payload, 'iterations');
      const workUnits = numberField(payload, 'completed_work_units') ?? numberField(payload, 'work_units');
      const limit = numberField(payload, 'work_unit_limit');
      const progress = limit != null ? ` · issues handled ${workUnits ?? 0} / ${limit}` : '';
      return {
        eventName,
        title: 'Loop stopped',
        detail: `reason ${stringField(payload, 'reason') ?? 'unknown'} · cycles ${cycles ?? '?'}${progress}`,
        chips: ['stopped'],
        tone: 'success'
      };
    }
    if (eventName === 'autopilot_cli_line') {
      const kind = stringField(payload, 'kind') ?? entry.stream;
      const fields = objectField(payload, 'fields') ?? {};
      const issue = stringField(fields, 'issue');
      const status = stringField(fields, 'status');
      const action = stringField(fields, 'action');
      return {
        eventName,
        title: kind === 'latest' ? 'Latest lane update' : kind,
        detail: [issue, status, action].filter(Boolean).join(' · ') || compactRunLine(stringField(payload, 'raw') ?? entry.line),
        chips: [kind, entry.stream],
        tone: entry.stream === 'stderr' ? 'warn' : 'info'
      };
    }

    return {
      eventName,
      title: eventName.replaceAll('_', ' '),
      detail: compactRunLine(entry.line),
      chips: [entry.stream, eventName],
      tone: 'info'
    };
  }

  function runLogTone(status: string, blockers: number) {
    const value = status.toLowerCase();
    if (value.includes('error') || value.includes('failed')) return 'error';
    if (value.includes('blocked') || blockers > 0) return 'warn';
    if (value.includes('completed') || value.includes('success') || value.includes('stopped')) return 'success';
    return 'info';
  }

  function compactRunLine(value: string) {
    const compact = value.trim().replace(/\s+/g, ' ');
    return compact.length > 180 ? `${compact.slice(0, 180)}...` : compact || 'No output text.';
  }

  function objectField(value: unknown, key: string): Record<string, unknown> | null {
    return objectFromUnknown(objectFromUnknown(value)[key]);
  }

  function objectFromUnknown(value: unknown): Record<string, unknown> {
    return value != null && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {};
  }

  function arrayField(value: unknown, key: string): unknown[] | null {
    const next = objectFromUnknown(value)[key];
    return Array.isArray(next) ? next : null;
  }

  function stringField(value: unknown, key: string): string | null {
    const next = objectFromUnknown(value)[key];
    return typeof next === 'string' && next.trim() ? next : null;
  }

  function numberField(value: unknown, key: string): number | null {
    const next = objectFromUnknown(value)[key];
    return typeof next === 'number' && Number.isFinite(next) ? next : null;
  }

  function booleanField(value: unknown, key: string): boolean | null {
    const next = objectFromUnknown(value)[key];
    return typeof next === 'boolean' ? next : null;
  }

  function maxConcurrentChip(value: Record<string, unknown>) {
    const maxConcurrent = numberField(value, 'max_concurrent');
    return maxConcurrent == null ? null : `max ${maxConcurrent}`;
  }

  function concurrencyChips(settings: Record<string, unknown> | null) {
    if (!settings) return [];
    return [
      laneLimitChip(settings, 'main_max_concurrent', 'main'),
      laneLimitChip(settings, 'review_max_concurrent', 'review'),
      laneLimitChip(settings, 'merge_max_concurrent', 'merge')
    ].filter(Boolean);
  }

  function laneLimitChip(settings: Record<string, unknown>, key: string, label: string) {
    const value = numberField(settings, key);
    return value == null ? null : `${label} max ${value}`;
  }

  function selectedIssuesLabel(values: unknown[] | null) {
    const identifiers = (values ?? [])
      .map((value) => selectedIssueLabel(objectFromUnknown(value)))
      .filter(Boolean);
    return identifiers.length ? `selected ${identifiers.join(', ')}` : 'selected none';
  }

  function selectedIssueLabel(issue: Record<string, unknown> | null) {
    if (!issue) return null;
    return stringField(issue, 'identifier') ?? stringField(issue, 'title');
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
    const openAutoloopLogsListener = () => (runLogsOpen = true);
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
        onOpenLogs={() => (logsOpen = true)}
        onOpenRunLogs={() => (runLogsOpen = true)}
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

{#if runLogsOpen}
  <div class="modal-backdrop">
    <button class="modal-scrim" type="button" aria-label="Close run log" onclick={() => (runLogsOpen = false)}></button>
    <div class="cli-log-modal autoloop-log-modal" role="dialog" aria-modal="true" aria-labelledby="autoloop-log-title">
      <header>
        <div>
          <p class="eyebrow">Developer Tools</p>
          <h2 id="autoloop-log-title">Run Logs</h2>
          <span>{autoloopState.mode} · {autoloopState.workflowPath}</span>
        </div>
        <button class="btn btn-ghost" type="button" onclick={() => (runLogsOpen = false)}>Close</button>
      </header>

      {#if autoloopStdoutLines.length}
        <div class="autoloop-stdout-list" aria-label="Run logs">
          {#each autoloopStdoutLines as entry, index}
            {@const summary = runLogSummary(entry)}
            <div class="autoloop-stdout-line">
              <time>{formatAutoloopTime(entry.atMs)}</time>
              <div>
                <button
                  class="cli-json-log-meta"
                  type="button"
                  aria-expanded={expandedRunLogRows.has(`stdout-${entry.atMs}-${index}`)}
                  onclick={() => toggleRunLogRow(`stdout-${entry.atMs}-${index}`)}
                >
                  <span class="json-tree-arrow" aria-hidden="true">›</span>
                  <span class="run-log-tone {summary.tone}" aria-hidden="true"></span>
                  <strong>{summary.title}</strong>
                  <span>{summary.eventName}</span>
                </button>
                <div class="run-log-human">
                  <p>{summary.detail}</p>
                  {#if summary.chips.length}
                    <div class="run-log-chips" aria-label="Run log summary">
                      {#each summary.chips as chip}
                        <span>{chip}</span>
                      {/each}
                    </div>
                  {/if}
                </div>
                {#if expandedRunLogRows.has(`stdout-${entry.atMs}-${index}`)}
                  <JsonLogView value={entry.event ?? entry.line} fallbackLabel="Autoloop stdout text" />
                {/if}
              </div>
            </div>
          {/each}
        </div>
      {:else}
        <div class="cli-log-empty">
          <strong>No run output yet</strong>
          <p>{tauriError || 'Start a dry run or write run to capture autoloop output here.'}</p>
        </div>
      {/if}
    </div>
  </div>
{/if}
