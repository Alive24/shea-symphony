<script lang="ts">
  import { onMount } from 'svelte';
  import {
    DATA_MODE_CHANGE_EVENT,
    HANDOFF_TARGETS,
    HANDOFF_TARGET_CHANGE_EVENT,
    buildViewModel,
    getDefaultHandoffTarget,
    loadOverview,
    loadReadSurface,
    mergeReadSurface
  } from './lib/api.ts';
  import {
    appendAutoloopLine,
    defaultLoopState,
    getLoopState,
    isTauriRuntime,
    laneWorkerFromAutoloop,
    mergeLaneSnapshot,
    startAutoloop,
    stopAutoloop,
    subscribeAutoloopEvents,
    type LaneSnapshot,
    type LoopStateSnapshot
  } from './lib/tauriAutoloop.ts';

  let loading = true;
  let liveError = '';
  let tauriError = '';
  let autoloopBusy = false;
  let tauriAvailable = false;
  let autoloopState: LoopStateSnapshot = defaultLoopState();
  let fullLoading = false;
  let backgroundRefreshing = false;
  let slowReadsRemaining = 0;
  let defaultHandoffTarget = 'codex-app';
  let copiedHandoffId = '';
  let view = buildViewModel(null);
  const autoRefreshMs = 45_000;

  $: dataSource = view.dataSource;
  $: queueIssues = view.queueIssues ?? [];
  $: liveUnavailable = dataSource?.mode === 'offline';
  $: autoloopLanes = autoloopState?.lanes ?? {};
  $: latestAutoloopLine = autoloopState?.recentLines?.slice(-1)[0]?.line ?? '';
  $: humanTodoIssues = queueIssues
    .filter((issue) => isHumanTodoState(issue.state))
    .map((issue) => ({
      ...issue,
      category: humanTodoCategory(issue.state),
      categoryDetail: humanTodoDetail(issue.state),
      categoryTone: humanTodoTone(issue.state)
    }));
  $: laneBoard = ['main', 'review', 'merge'].map((laneKey) => {
    const label = titleCaseLane(laneKey);
    const workers = view.laneWorkers?.[laneKey] ?? [];
    const liveWorker = laneWorkerFromAutoloop(autoloopLanes[laneKey], laneKey, autoloopState);
    const visibleWorkers = liveWorker ? [liveWorker, ...workers] : workers;
    const queued = queueIssues.filter((issue) => issue.lane === label);
    const workerIssues = visibleWorkers.map((worker, index) => ({
      kind: 'picked',
      id: normalizeIssueRef(worker.issue) ?? worker.issue ?? `worker-${index + 1}`,
      title: worker.title ?? worker.action ?? 'Worker active',
      meta: `${worker.action ?? 'Active'} · ${worker.backend ?? 'worker'} · ${worker.session ?? worker.elapsed ?? 'session'}`,
      tone: 'success',
      workerNumber: index + 1
    }));
    const pickedIssueIds = new Set(workerIssues.map((issue) => normalizeIssueRef(issue.id)).filter(Boolean));
    const waitingIssues = queued
      .filter((issue) => !pickedIssueIds.has(normalizeIssueRef(issue.id)))
      .map((issue) => ({
        kind: 'queued',
        id: issue.id,
        title: issue.title,
        meta: `${issue.state} · Next Skill: ${issue.nextSkill}`,
        tone: issue.tone,
        workerNumber: null
      }));
    return {
      laneKey,
      label,
      issues: [...workerIssues, ...waitingIssues],
      pickedCount: visibleWorkers.length,
      queuedCount: waitingIssues.length,
      tone: visibleWorkers.length ? 'success' : waitingIssues.length ? 'warn' : 'neutral'
    };
  });

  async function refresh(force = false, includeSlowReads = true, background = false) {
    if (background) {
      backgroundRefreshing = true;
    } else {
      loading = true;
      fullLoading = false;
    }
    slowReadsRemaining = 0;
    liveError = '';
    try {
      view = buildViewModel(await loadOverview(force, 'fast'));
      loading = false;
      if (!includeSlowReads) return;
      fullLoading = true;
      const slowSurfaces = ['autopilot', 'doctor', 'review', 'local'];
      slowReadsRemaining = slowSurfaces.length;
      await Promise.allSettled(
        slowSurfaces.map(async (name) => {
          try {
            const surface = await loadReadSurface(name, force);
            view = buildViewModel(mergeReadSurface(view.raw, surface));
          } finally {
            slowReadsRemaining -= 1;
          }
        })
      );
    } catch (error) {
      liveError = error.message;
      view = buildViewModel(null);
    } finally {
      if (!background) loading = false;
      fullLoading = false;
      backgroundRefreshing = false;
      slowReadsRemaining = 0;
    }
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

  function titleCaseLane(value) {
    return String(value ?? '')
      .replace(/[-_]/g, ' ')
      .replace(/\b\w/g, (letter) => letter.toUpperCase());
  }

  function normalizeIssueRef(value) {
    const match = String(value ?? '').match(/#?(\d+)/);
    return match ? `#${match[1]}` : null;
  }

  function handoffLabel(targetId) {
    return HANDOFF_TARGETS.find((target) => target.id === targetId)?.label ?? 'Codex App';
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
    if (defaultHandoffTarget === 'github' && issue.url) {
      window.open(issue.url, '_blank', 'noreferrer');
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

  async function startDryRunAutoloop() {
    autoloopBusy = true;
    tauriError = '';
    try {
      autoloopState = await startAutoloop({
        workflowPath: 'workflows/shea-symphony.md',
        maxIterations: 1,
        write: false
      });
    } catch (error) {
      tauriError = error.message;
    } finally {
      autoloopBusy = false;
    }
  }

  async function stopRunningAutoloop() {
    autoloopBusy = true;
    tauriError = '';
    try {
      autoloopState = await stopAutoloop();
    } catch (error) {
      tauriError = error.message;
    } finally {
      autoloopBusy = false;
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
      } else if (event.type === 'snapshot') {
        autoloopState = event.payload;
      } else if (event.type === 'started' || event.type === 'stopped' || event.type === 'error') {
        refreshAutoloopState();
      }
    }).then((unlisten) => {
      unlistenAutoloop = unlisten;
    });
    refresh();
    const dataModeListener = () => refresh(true);
    const handoffTargetListener = (event) => {
      defaultHandoffTarget = event.detail?.target ?? getDefaultHandoffTarget();
    };
    window.addEventListener(DATA_MODE_CHANGE_EVENT, dataModeListener);
    window.addEventListener(HANDOFF_TARGET_CHANGE_EVENT, handoffTargetListener);
    const autoRefresh = window.setInterval(() => {
      if (loading || fullLoading || backgroundRefreshing || document.visibilityState !== 'visible') return;
      refresh(false, false, true);
    }, autoRefreshMs);
    const handoffRefresh = window.setInterval(() => {
      const nextTarget = getDefaultHandoffTarget();
      if (nextTarget !== defaultHandoffTarget) {
        defaultHandoffTarget = nextTarget;
      }
    }, 300);
    return () => {
      window.removeEventListener(DATA_MODE_CHANGE_EVENT, dataModeListener);
      window.removeEventListener(HANDOFF_TARGET_CHANGE_EVENT, handoffTargetListener);
      window.clearInterval(autoRefresh);
      window.clearInterval(handoffRefresh);
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
  <section class="human-todo-overview">
    <div class="human-todo-rail" aria-label="Human operator issue queue">
      {#if humanTodoIssues.length}
        {#each humanTodoIssues as issue}
          <article class="human-todo-card {issue.categoryTone}">
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
              <button class="btn btn-primary" type="button" onclick={() => openHandoff(issue)}>
                Open in {handoffLabel(defaultHandoffTarget)}
              </button>
              <button class="btn btn-ghost" type="button" onclick={() => copyHandoffPrompt(issue)}>
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
            {liveUnavailable
              ? 'Waiting for live Project readback before showing operator-owned issues.'
              : 'The current Project read did not surface Need to Clarify, Need Human Input, or Human Review items.'}
          </p>
        </article>
      {/if}
    </div>
  </section>

  <section class="lane-board-overview" aria-label="Worker pickup and queue by lane">
    <div class="autoloop-control-bar" aria-label="Autoloop controls">
      <div>
        <strong>{autoloopState.running ? 'Autoloop running' : 'Autoloop idle'}</strong>
        <span>
          {tauriAvailable ? `${autoloopState.mode} · ${autoloopState.workflowPath}` : 'Open in Shea Symphony App desktop shell for live loop control.'}
        </span>
        {#if latestAutoloopLine}
          <small>{latestAutoloopLine}</small>
        {/if}
      </div>
      <div>
        <button class="btn btn-primary" type="button" disabled={!tauriAvailable || autoloopBusy || autoloopState.running} onclick={startDryRunAutoloop}>
          Start dry-run
        </button>
        <button class="btn btn-ghost" type="button" disabled={!tauriAvailable || autoloopBusy || !autoloopState.running} onclick={stopRunningAutoloop}>
          Stop
        </button>
      </div>
    </div>
    <div class="lane-board-grid">
      {#each laneBoard as lane}
        <article class="lane-board-column {lane.tone}">
          <div class="lane-board-column-head">
            <strong>{lane.label}</strong>
          </div>

          <div class="lane-board-issue-list">
            {#if lane.issues.length}
              {#each lane.issues as issue}
                <div class="lane-board-item {issue.kind === 'picked' ? 'picked' : issue.tone}">
                  {#if issue.kind === 'picked'}
                    <span class="worker-number">{issue.workerNumber}</span>
                  {:else}
                    <span class="worker-number placeholder" aria-hidden="true"></span>
                  {/if}
                  <strong>{issue.id}</strong>
                  <span>{issue.title}</span>
                </div>
              {/each}
            {:else}
              <div class="lane-board-empty">No issue visible.</div>
            {/if}
          </div>
        </article>
      {/each}
    </div>
  </section>
</section>
