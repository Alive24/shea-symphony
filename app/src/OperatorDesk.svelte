<script lang="ts">
  import { onMount } from 'svelte';
  import {
    HANDOFF_TARGETS,
    HANDOFF_TARGET_CHANGE_EVENT,
    autoloopControlStore,
    autoloopStateStore,
    getDefaultHandoffTarget,
    refreshStatusStore
  } from './lib/uiState.ts';
  import { operatorOverviewStore, requestOperatorLocalArtifactsRefresh } from './lib/operatorOverviewStore.ts';
  import { buildLaneThroughputBoard } from './lib/viewModel/laneThroughput.ts';
  import {
    appendAutoloopLine,
    defaultLoopState,
    getLoopState,
    isTauriRuntime,
    laneWorkerFromAutoloop,
    laneWorkersFromAutoloopLines,
    mergeLaneSnapshot,
    subscribeAutoloopEvents,
    type LaneSnapshot,
    type LoopStateSnapshot,
    type AutoloopLine
  } from './lib/tauriAutoloop.ts';

  let tauriError = '';
  let autoloopBusy = false;
  let tauriAvailable = false;
  let autoloopState: LoopStateSnapshot = defaultLoopState();
  let defaultHandoffTarget = 'codex-app';
  let copiedHandoffId = '';
  let autoloopRefreshTimer: number | null = null;
  let lastStableHumanTodoIssues = [];
  let lastStableLaneBoard = [];

  $: view = $operatorOverviewStore.view;
  $: liveError = $operatorOverviewStore.liveError;
  $: fullLoading = $operatorOverviewStore.fullLoading;
  $: slowReadsRemaining = $operatorOverviewStore.slowReadsRemaining;
  $: dataSource = view.dataSource;
  $: queueIssues = view.queueIssues ?? [];
  $: liveUnavailable = dataSource?.mode === 'offline';
  $: autoloopLanes = autoloopState?.lanes ?? {};
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
    laneMaxSummary: laneMaxSummary(autoloopLanes)
  });
  $: autoloopStateStore.set(autoloopState);
  $: operatorSurfaceRefreshing = $refreshStatusStore.running;
  $: issueTitleById = buildIssueTitleMap(queueIssues);
  $: liveWorkersByLane = ['main', 'review', 'merge'].reduce((lanes, laneKey) => {
    const liveWorker = laneWorkerFromAutoloop(autoloopLanes[laneKey], laneKey, autoloopState);
    const liveLogWorkers = laneWorkersFromAutoloopLines(autoloopState, laneKey);
    lanes[laneKey] = [...(liveWorker ? [liveWorker] : []), ...liveLogWorkers];
    return lanes;
  }, {});
  $: humanTodoIssues = queueIssues
    .filter((issue) => isHumanTodoState(issue.state))
    .map((issue) => ({
      ...issue,
      category: humanTodoCategory(issue.state),
      categoryDetail: humanTodoDetail(issue.state),
      categoryTone: humanTodoTone(issue.state)
    }));
  $: currentLaneBoard = buildLaneThroughputBoard({
    queueIssues,
    laneWorkers: view.laneWorkers,
    liveWorkersByLane,
    laneSnapshots: autoloopLanes,
    issueTitleById,
    fullLoading
  });
  $: if (!operatorSurfaceRefreshing) {
    lastStableHumanTodoIssues = humanTodoIssues;
    lastStableLaneBoard = currentLaneBoard;
  }
  $: visibleHumanTodoIssues =
    operatorSurfaceRefreshing && lastStableHumanTodoIssues.length
      ? lastStableHumanTodoIssues.map((issue) => ({ ...issue, refreshing: true }))
      : humanTodoIssues.map((issue) => ({ ...issue, refreshing: operatorSurfaceRefreshing }));
  $: laneBoard =
    operatorSurfaceRefreshing && lastStableLaneBoard.length
      ? lastStableLaneBoard.map((lane) => ({ ...lane, refreshing: true }))
      : currentLaneBoard.map((lane) => ({ ...lane, refreshing: operatorSurfaceRefreshing }));

  function scheduleAutoloopLocalRefresh(source = 'autoloop') {
    if ($refreshStatusStore.running) return;
    if (autoloopRefreshTimer) window.clearTimeout(autoloopRefreshTimer);
    autoloopRefreshTimer = window.setTimeout(() => {
      autoloopRefreshTimer = null;
      requestOperatorLocalArtifactsRefresh(source, false);
    }, 900);
  }

  function buildIssueTitleMap(issues) {
    const titles = new Map();
    for (const issue of issues ?? []) {
      const key = normalizeIssueRef(issue.id);
      if (key && issue.title) titles.set(key, issue.title);
    }
    return titles;
  }

  function laneMaxSummary(lanes) {
    const parts = ['main', 'review', 'merge']
      .map((laneKey) => {
        const value = Number(lanes?.[laneKey]?.maxConcurrent);
        return Number.isFinite(value) ? `${laneKey} ${value}` : null;
      })
      .filter(Boolean);
    return parts.length ? `max · ${parts.join(' · ')}` : '';
  }

  function isHumanTodoState(state) {
    return ['Need to Clarify', 'Need Human Input', 'Human Review'].includes(state);
  }

  function humanTodoCategory(state) {
    if (state === 'Need to Clarify') return 'Need to Clarify';
    if (state === 'Need Human Input') return 'Need Human Input';
    return 'Human Review';
  }

  function humanTodoDetail(state) {
    if (state === 'Need to Clarify') return 'Issue contract is not executable yet.';
    if (state === 'Need Human Input') return 'Work cannot continue without human input.';
    return 'Review passed; waiting on human approval.';
  }

  function humanTodoTone(state) {
    if (state === 'Human Review') return 'project-pink';
    return 'project-yellow';
  }

  function assigneeLabel(issue) {
    const assignees = Array.isArray(issue.assignees) ? issue.assignees.filter(Boolean) : [];
    if (!assignees.length) return 'Unassigned';
    if (assignees.length === 1) return assignees[0];
    return `${assignees[0]} +${assignees.length - 1}`;
  }

  function normalizeIssueRef(value) {
    const match = String(value ?? '').match(/#?(\d+)/);
    return match ? `#${match[1]}` : null;
  }

  function handoffLabel(targetId) {
    return HANDOFF_TARGETS.find((target) => target.id === targetId)?.label ?? 'Codex App';
  }

  function latestAutoloopStdout(state: LoopStateSnapshot, lines: AutoloopLine[]) {
    const startedAt = Number(state.startedAtMs);
    const lowerBound = Number.isFinite(startedAt) ? startedAt - 1000 : null;
    return lines.filter((entry) => entry.stream === 'stdout' && (lowerBound == null || entry.atMs >= lowerBound));
  }

  function handoffPrompt(issue) {
    return [
      `Use the appropriate Shea Symphony Skill for ${issue.id}.`,
      '',
      `Issue: ${issue.id} ${issue.title}`,
      `State: ${issue.state}`,
      `Lane: ${issue.lane}`,
      `Category: ${issue.category}`,
      `Recommended: ${issue.recommended}`,
      issue.url ? `URL: ${issue.url}` : '',
      '',
      'Read the current Project issue state first, preserve lane boundaries, and ask before any Project mutation.'
    ]
      .filter(Boolean)
      .join('\n');
  }

  async function copyHandoffPrompt(issue) {
    try {
      await navigator.clipboard.writeText(handoffPrompt(issue));
      copiedHandoffId = issue.id;
      window.setTimeout(() => {
        if (copiedHandoffId === issue.id) copiedHandoffId = '';
      }, 1800);
    } catch (_) {
      copiedHandoffId = '';
    }
  }

  async function openHandoff(issue) {
    await copyHandoffPrompt(issue);
  }

  async function refreshAutoloopState() {
    try {
      tauriAvailable = isTauriRuntime();
      autoloopState = await getLoopState();
    } catch (error) {
      tauriError = error.message;
    }
  }

  onMount(() => {
    defaultHandoffTarget = getDefaultHandoffTarget();
    refreshAutoloopState();
    let unlistenAutoloop: (() => void) | undefined;
    subscribeAutoloopEvents((event) => {
      if (event.type === 'line') {
        autoloopState = appendAutoloopLine(autoloopState, event.payload);
      } else if (event.type === 'lane') {
        autoloopState = mergeLaneSnapshot(autoloopState, event.payload);
        scheduleAutoloopLocalRefresh('autoloop-lane');
      } else if (event.type === 'snapshot') {
        autoloopState = event.payload;
      } else if (event.type === 'started' || event.type === 'stopped' || event.type === 'error') {
        refreshAutoloopState();
        if (event.type !== 'started') scheduleAutoloopLocalRefresh(`autoloop-${event.type}`);
      }
    }).then((unlisten) => {
      unlistenAutoloop = unlisten;
    });
    const handoffTargetListener = (event) => {
      defaultHandoffTarget = event.detail?.target ?? getDefaultHandoffTarget();
    };
    window.addEventListener(HANDOFF_TARGET_CHANGE_EVENT, handoffTargetListener);
    const handoffRefresh = window.setInterval(() => {
      const nextTarget = getDefaultHandoffTarget();
      if (nextTarget !== defaultHandoffTarget) {
        defaultHandoffTarget = nextTarget;
      }
    }, 300);
    return () => {
      window.removeEventListener(HANDOFF_TARGET_CHANGE_EVENT, handoffTargetListener);
      autoloopControlStore.set({
        tauriAvailable: false,
        busy: false,
        running: false,
        mode: 'dry-run',
        workflowPath: 'workflows/shea-symphony.md',
        latestLine: 'No recent autoloop result',
        laneMaxSummary: ''
      });
      window.clearInterval(handoffRefresh);
      if (autoloopRefreshTimer) window.clearTimeout(autoloopRefreshTimer);
      unlistenAutoloop?.();
    };
  });
</script>

{#if liveError}
  <div class="inline-alert">
    <strong>Live API unavailable</strong>
    <span>{liveError}</span>
  </div>
{/if}

{#if tauriError}
  <div class="inline-alert">
    <strong>Tauri bridge unavailable</strong>
    <span>{tauriError}</span>
  </div>
{/if}

<section class="operator-first-screen" aria-label="Operator first screen">
  <section class:refreshing={operatorSurfaceRefreshing} class="human-todo-overview" aria-busy={operatorSurfaceRefreshing}>
    <div class="human-todo-rail" aria-label="Human operator issue queue">
      {#if visibleHumanTodoIssues.length}
        {#each visibleHumanTodoIssues as issue}
          <article class="human-todo-card {issue.categoryTone}" class:refreshing={issue.refreshing}>
            <div class="human-todo-card-head">
              <div class="human-todo-identity">
                <span class="issue-tag">{issue.id}</span>
                <span class="assignee-pill">{assigneeLabel(issue)}</span>
              </div>
              <span class="human-todo-type {issue.categoryTone}">{issue.category}</span>
            </div>
            <div>
              <strong>{issue.title}</strong>
              <p>{issue.categoryDetail}</p>
            </div>
            <div class="human-todo-meta">
              <span>{issue.lane} · {issue.workerStatus}</span>
              <small>{issue.recommended}</small>
            </div>
            <div class="handoff-actions">
              <button class="btn btn-primary" type="button" disabled={operatorSurfaceRefreshing} onclick={() => openHandoff(issue)}>
                Open in {handoffLabel(defaultHandoffTarget)}
              </button>
              <button class="btn btn-ghost" type="button" disabled={operatorSurfaceRefreshing} onclick={() => copyHandoffPrompt(issue)}>
                {copiedHandoffId === issue.id ? 'Copied' : 'Copy Handoff Prompt'}
              </button>
            </div>
          </article>
        {/each}
      {:else}
        <article class="human-todo-empty">
          <span class="issue-tag">Clear</span>
          <strong>No human to-do issues visible</strong>
          <p>
            {fullLoading
              ? `Loading CLI readback... ${slowReadsRemaining} surface${slowReadsRemaining === 1 ? '' : 's'} remaining.`
              : liveUnavailable
              ? 'Waiting for live Project readback before showing operator-owned issues.'
              : 'The current Project read did not surface Need to Clarify, Need Human Input, or Human Review items.'}
          </p>
        </article>
      {/if}
    </div>
  </section>

  <section class:refreshing={operatorSurfaceRefreshing} class="lane-board-overview" aria-label="Worker pickup and queue by lane" aria-busy={operatorSurfaceRefreshing}>
    <div class="autoloop-control-bar" aria-label="Autoloop controls">
      <div>
        <strong>{autoloopState.running ? 'Autoloop running' : 'Autoloop idle'}</strong>
        <span>
          {tauriAvailable ? `${autoloopState.mode} · ${autoloopState.workflowPath}` : 'Open in Shea Symphony App desktop shell for live loop control.'}
        </span>
        {#if latestAutoloopLine}
          <small>{latestAutoloopLine}</small>
        {:else if fullLoading}
          <small>Loading CLI readback · {slowReadsRemaining} surface{slowReadsRemaining === 1 ? '' : 's'} remaining</small>
        {/if}
      </div>
    </div>
    <div class="lane-board-grid">
      {#each laneBoard as lane}
        <article class="lane-board-column {lane.tone}">
          <div class="lane-board-column-head compact">
            <strong>{lane.label}</strong>
            <span
              class="lane-board-state-slot {lane.refreshing || (fullLoading && !lastStableLaneBoard.length) ? 'loading' : lane.status}"
              aria-label={lane.refreshing || (fullLoading && !lastStableLaneBoard.length)
                ? `${lane.label} loading`
                : lane.status === 'complete'
                ? `${lane.label} complete`
                : `${lane.label} ${lane.status}`}
            >
              {#if lane.refreshing || (fullLoading && !lastStableLaneBoard.length)}
                <span class="lane-board-spinner" aria-hidden="true"></span>
              {:else if lane.status === 'complete'}
                <span aria-hidden="true">✓</span>
              {:else if lane.status === 'blocked'}
                <span aria-hidden="true">!</span>
              {:else}
                <span aria-hidden="true"></span>
              {/if}
            </span>
          </div>

          <div class="lane-board-issue-list">
            {#if lane.issues.length}
              {#each lane.issues as issue}
                <div class="lane-board-item {issue.kind === 'picked' ? 'picked' : issue.tone} {issue.waiting ? 'waiting' : ''}">
                  {#if issue.kind === 'picked'}
                    <span class="worker-number {issue.waiting ? 'waiting' : ''}">{issue.workerNumber}</span>
                  {:else}
                    <span class="worker-number placeholder" aria-hidden="true"></span>
                  {/if}
                  <strong>{issue.id}</strong>
                  <span>
                    {issue.title}
                    {#if issue.meta}
                      <small>{issue.meta}</small>
                    {/if}
                  </span>
                </div>
              {/each}
            {:else}
              <div class="lane-board-empty">{fullLoading && !lane.refreshing && !lastStableLaneBoard.length ? 'Loading CLI readback...' : 'No issue visible.'}</div>
            {/if}
          </div>
        </article>
      {/each}
    </div>
  </section>
</section>
