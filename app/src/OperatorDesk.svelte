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
  import AttentionCard from './lib/AttentionCard.svelte';
  import LaneBoard from './lib/LaneBoard.svelte';
  import { operatorOverviewStore, requestOperatorLocalArtifactsRefresh } from './lib/operatorOverviewStore.ts';
  import { humanTodoRefreshState } from './lib/viewModel/humanTodoRefresh.ts';
  import { buildLaneThroughputBoard } from './lib/viewModel/laneThroughput.ts';
  import { buildHandoffPrompt } from './lib/viewModel/handoffPrompt.ts';
  import {
    appendAutoloopLine,
    defaultLoopState,
    getLoopState,
    isTauriRuntime,
    laneWorkerFromAutoloop,
    laneWorkersFromAutoloopLines,
    mergeLaneSnapshot,
    openCodexHandoff,
    operatorRunLogLines,
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
  let handoffStatus = {};
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
  $: hasProjectQueueRead = hasReadableProjectQueue(view.raw);
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
  $: humanTodoEmptyState = humanTodoRefreshState({
    visibleIssueCount: visibleHumanTodoIssues.length,
    fullLoading,
    slowReadsRemaining,
    operatorSurfaceRefreshing,
    liveUnavailable,
    hasProjectQueueRead
  });
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

  function normalizeIssueRef(value) {
    const match = String(value ?? '').match(/#?(\d+)/);
    return match ? `#${match[1]}` : null;
  }

  function hasReadableProjectQueue(overview) {
    const command = overview?.commands?.githubQueue;
    if (command?.ok) return true;
    const queue = overview?.githubQueue;
    return Array.isArray(queue?.issues) || queue?.stateCounts || queue?.laneCounts;
  }

  function handoffLabel(targetId) {
    return HANDOFF_TARGETS.find((target) => target.id === targetId)?.label ?? 'Codex App';
  }

  function latestAutoloopStdout(state: LoopStateSnapshot, lines: AutoloopLine[]) {
    return operatorRunLogLines(state, lines);
  }

  function handoffMessage(issue) {
    return handoffStatus[issue.id] ?? '';
  }

  async function copyHandoffPrompt(issue) {
    try {
      await navigator.clipboard.writeText(buildHandoffPrompt(issue));
      copiedHandoffId = issue.id;
      handoffStatus = { ...handoffStatus, [issue.id]: '' };
      window.setTimeout(() => {
        if (copiedHandoffId === issue.id) copiedHandoffId = '';
      }, 1800);
      return true;
    } catch (_) {
      copiedHandoffId = '';
      handoffStatus = { ...handoffStatus, [issue.id]: 'Clipboard unavailable; prompt was not copied.' };
      return false;
    }
  }

  function issueWorktreePath(issue) {
    const issueRef = normalizeIssueRef(issue?.id);
    const localStatus = view?.raw?.localStatus ?? {};
    const candidates = [
      ...(localStatus.issueWorktrees ?? []),
      ...(localStatus.completedIssueWorktrees ?? [])
    ];
    return candidates.find((entry) => normalizeIssueRef(entry?.issue ?? entry?.issueRef ?? entry?.id) === issueRef)?.path ?? null;
  }

  async function openHandoff(issue) {
    const prompt = buildHandoffPrompt(issue);
    if (defaultHandoffTarget !== 'codex-app') {
      const copied = await copyHandoffPrompt(issue);
      handoffStatus = {
        ...handoffStatus,
        [issue.id]: copied ? '' : `Clipboard unavailable. Open ${handoffLabel(defaultHandoffTarget)} manually after copying the prompt.`
      };
      return;
    }
    try {
      const worktreePath = issueWorktreePath(issue);
      if (!worktreePath) {
        throw new Error('No local issue worktree is visible. Refresh local artifacts before opening Codex.');
      }
      await openCodexHandoff(prompt, worktreePath);
      handoffStatus = { ...handoffStatus, [issue.id]: '' };
    } catch (error) {
      handoffStatus = {
        ...handoffStatus,
        [issue.id]: error instanceof Error ? error.message : 'Unable to open Codex handoff.'
      };
    }
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
          <AttentionCard
            {issue}
            disabled={operatorSurfaceRefreshing}
            handoffTargetLabel={handoffLabel(defaultHandoffTarget)}
            copied={copiedHandoffId === issue.id}
            message={handoffMessage(issue)}
            onOpen={openHandoff}
            onCopy={copyHandoffPrompt}
          />
        {/each}
      {:else}
        <article class="human-todo-empty {humanTodoEmptyState.status}" aria-busy={!humanTodoEmptyState.isClear}>
          <span class="issue-tag">{humanTodoEmptyState.badge}</span>
          <strong>{humanTodoEmptyState.title}</strong>
          <p>{humanTodoEmptyState.detail}</p>
        </article>
      {/if}
    </div>
  </section>

  <LaneBoard
    lanes={laneBoard}
    refreshing={operatorSurfaceRefreshing}
    {fullLoading}
    hasStableLanes={lastStableLaneBoard.length > 0}
    autoloopRunning={autoloopState.running}
    {tauriAvailable}
    autoloopMode={autoloopState.mode}
    workflowPath={autoloopState.workflowPath}
    {latestAutoloopLine}
    {slowReadsRemaining}
  />
</section>
