<script lang="ts">
  import { onMount } from 'svelte';
  import IssueIndex from '$lib/IssueIndex.svelte';
  import LaneCard from '$lib/LaneCard.svelte';
  import { DATA_MODE_CHANGE_EVENT, buildViewModel, loadOverview, loadReadSurface, mergeReadSurface } from '$lib/api';

  let view = buildViewModel(null);
  let loading = true;
  let backgroundRefreshing = false;
  let slowReadsRemaining = 0;
  const autoRefreshMs = 45_000;
  $: laneSummaries = view.laneSummaries;
  $: stateDistribution = view.stateDistribution ?? [];
  $: issueIndex = view.issueIndex ?? [];
  $: projectWorkerMatch = view.projectWorkerMatch;
  $: matchRows = projectWorkerMatch?.lanes ?? [];
  $: stateMax = Math.max(1, ...stateDistribution.map((item) => item.count));
  $: laneBoundaries = [
    { lane: 'Main', entry: 'Todo or Rework', exit: 'Agent Review', risk: 'Implementation must not self-review.' },
    { lane: 'Review', entry: 'Agent Review', exit: 'Human Review or Rework', risk: 'Findings must stay independent and evidenced.' },
    { lane: 'Human', entry: 'Human Review', exit: 'Merging or Rework', risk: 'Routing requires explicit operator decision.' },
    { lane: 'Merge', entry: 'Merging', exit: 'Done or Need Human Input', risk: 'Unsafe conflicts stop instead of blending lanes.' }
  ];

  async function refresh(includeSlowReads = true, background = false) {
    if (background) {
      backgroundRefreshing = true;
    } else {
      loading = true;
    }
    slowReadsRemaining = 0;
    try {
      view = buildViewModel(await loadOverview(false, 'fast'));
      loading = false;
      if (!includeSlowReads) return;
      const slowSurfaces = ['autopilot', 'doctor', 'review', 'local'];
      slowReadsRemaining = slowSurfaces.length;
      await Promise.allSettled(
        slowSurfaces.map(async (name) => {
          try {
            const surface = await loadReadSurface(name, false);
            view = buildViewModel(mergeReadSurface(view.raw, surface));
          } finally {
            slowReadsRemaining -= 1;
          }
        })
      );
    } catch (error) {
      view = buildViewModel(null);
    } finally {
      if (!background) loading = false;
      backgroundRefreshing = false;
      slowReadsRemaining = 0;
    }
  }

  onMount(() => {
    refresh();
    const dataModeListener = () => refresh();
    window.addEventListener(DATA_MODE_CHANGE_EVENT, dataModeListener);
    const autoRefresh = window.setInterval(() => {
      if (loading || backgroundRefreshing || document.visibilityState !== 'visible') return;
      refresh(false, true);
    }, autoRefreshMs);
    return () => {
      window.removeEventListener(DATA_MODE_CHANGE_EVENT, dataModeListener);
      window.clearInterval(autoRefresh);
    };
  });
</script>

<section class="route-hero">
  <div>
    <p class="eyebrow">Lane control</p>
    <h2>Lanes</h2>
    <p>
      Main, Review, and Merge stay separated. Rework is treated as a state to resolve,
      not as another operator queue.
    </p>
  </div>
  <div class="hero-actions">
    <span class="section-note">{loading ? 'Fast readback' : backgroundRefreshing ? 'Auto-read now' : slowReadsRemaining ? `${slowReadsRemaining} live reads loading · auto 45s` : `${view.generatedAtLabel} · auto 45s`}</span>
    <a class="btn btn-primary" href="/lanes/main">Open Main lane</a>
  </div>
</section>

<section class="lane-grid expanded">
  {#each laneSummaries as lane}
    <LaneCard {lane} />
  {/each}
</section>

<section class="lane-observatory" aria-labelledby="lane-observatory-title">
  <div class="section-heading">
    <div>
      <p class="eyebrow">Lane Observatory</p>
      <h2 id="lane-observatory-title">State Pressure</h2>
    </div>
    <span class="section-note">Derived from overview and parked queues</span>
  </div>

  <div class="lane-observatory-grid">
    <article class="lane-match-overview {projectWorkerMatch?.tone ?? 'neutral'}">
      <div class="match-overview-head">
        <div>
          <span class="mini-label">Project / worker match</span>
          <h3>{projectWorkerMatch?.summary ?? '0/0 matched'}</h3>
        </div>
        <strong>{projectWorkerMatch?.label ?? 'Unknown'}</strong>
      </div>
      <p>{projectWorkerMatch?.detail ?? 'Waiting for live lane and worker readback.'}</p>
      <div class="lane-match-rows">
        {#each matchRows as row}
          <a class:warn={row.waiting || row.extraWorkers} class="lane-match-row" href={`/lanes/${row.lane.toLowerCase()}`}>
            <span>{row.lane}</span>
            <strong>{row.matched}/{row.project}</strong>
            <small>{row.project} Project · {row.workers} workers · {row.waiting} waiting</small>
          </a>
        {/each}
      </div>
    </article>

    <article class="target-chart wide">
      <span class="mini-label">Visible states</span>
      {#each stateDistribution as row}
        <div>
          <span>{row.state}</span>
          <meter min="0" max={stateMax} value={row.count}>
            {row.count}
          </meter>
          <strong>{row.count}</strong>
        </div>
      {/each}
    </article>

    <article class="boundary-mini">
      <span class="mini-label">Boundaries</span>
      {#each laneBoundaries as item}
        <div>
          <strong>{item.lane}</strong>
          <p>{item.entry} -> {item.exit}</p>
          <small>{item.risk}</small>
        </div>
      {/each}
    </article>
  </div>
</section>

<IssueIndex issues={issueIndex} />

<section class="quiet-explainer">
  <div>
    <span class="mini-label">Boundary</span>
    <h3>Worker details live one layer down</h3>
  </div>
  <p>
    The desk answers what needs human attention. Lane pages carry worker cards,
    session evidence, elapsed time, and target transitions for operators who need
    to inspect the machinery.
  </p>
</section>
