<script lang="ts">
  export let title: string;
  export let description: string;
  export let workers: any[] = [];
  export let projectItems: any[] = [];
  export let generatedAtLabel = 'not checked';
  export let backgroundRefreshing = false;

  const perPage = 5;
  let currentPage = 1;

  $: totalPages = Math.max(1, Math.ceil(workers.length / perPage));
  $: start = (currentPage - 1) * perPage;
  $: visibleWorkers = workers.slice(start, start + perPage);
  $: workerStart = workers.length ? start + 1 : 0;
  $: workerEnd = workers.length ? Math.min(start + perPage, workers.length) : 0;
  $: targetCounts = workers.reduce((counts, worker) => {
    const target = worker.target ?? 'Unknown';
    counts[target] = (counts[target] ?? 0) + 1;
    return counts;
  }, {});
  $: targetRows = Object.entries(targetCounts).slice(0, 5);
  $: primaryWorker = workers[0];
  $: primaryProjectItem = projectItems[0];
  $: evidenceRail = workers.slice(0, 6);
  $: projectStateCounts = projectItems.reduce((counts, item) => {
    const state = item.state ?? 'Unknown';
    counts[state] = (counts[state] ?? 0) + 1;
    return counts;
  }, {});
  $: projectStateRows = Object.entries(projectStateCounts).slice(0, 5);
  $: unavailableProjectItems = projectItems.filter((item) => item.workerStatus === 'Worker read unavailable');
  $: waitingProjectItems = projectItems.filter((item) => item.workerStatus === 'No worker visible');
  $: matchedProjectItems = projectItems.filter((item) => item.workerStatus === 'Worker matched');
  $: extraWorkerCount = Math.max(0, workers.length - matchedProjectItems.length);
  $: matchTone = unavailableProjectItems.length || waitingProjectItems.length || extraWorkerCount
    ? 'warn'
    : workers.length || projectItems.length
      ? 'success'
      : 'neutral';
  $: matchLabel = unavailableProjectItems.length
    ? 'Worker read unavailable'
    : waitingProjectItems.length
      ? 'Project waiting, no worker'
      : extraWorkerCount
        ? 'Worker without Project item'
        : workers.length || projectItems.length
          ? 'Project and worker aligned'
          : 'Idle and empty';
  $: matchDetail = unavailableProjectItems.length
    ? 'Worker session surface is unavailable, so Project items cannot be matched to current workers.'
    : waitingProjectItems.length
      ? `${waitingProjectItems.length} Project item${waitingProjectItems.length === 1 ? ' has' : 's have'} no current worker.`
      : extraWorkerCount
        ? `${extraWorkerCount} visible worker${extraWorkerCount === 1 ? '' : 's'} do not match a Project item in this lane.`
        : projectItems.length || workers.length
          ? 'Visible workers match Project items in this lane.'
          : 'No Project lane item or worker is visible.';

  function previous() {
    currentPage = Math.max(1, currentPage - 1);
  }

  function next() {
    currentPage = Math.min(totalPages, currentPage + 1);
  }
</script>

<section class="route-hero compact">
  <div>
    <p class="eyebrow">Secondary drilldown</p>
    <h2>{title}</h2>
    <p>{description}</p>
  </div>

  <div class="pagination">
    <span class="section-note">{backgroundRefreshing ? 'Auto-read now' : `${generatedAtLabel} · auto 45s`}</span>
    <button class="btn btn-ghost" type="button" on:click={previous} disabled={currentPage === 1}>Previous</button>
    <span>Workers {workerStart}-{workerEnd} of {workers.length}</span>
    <button class="btn btn-ghost" type="button" on:click={next} disabled={currentPage === totalPages}>Next</button>
  </div>
</section>

<section class="lane-visuals" aria-label={`${title} visual summary`}>
  <article class="lane-focus">
    <span class="mini-label">Current focus</span>
    {#if primaryWorker}
      <strong>{primaryWorker.issue}</strong>
      <p>{primaryWorker.action}</p>
      <small>{primaryWorker.evidence}</small>
    {:else if primaryProjectItem}
      <strong>{primaryProjectItem.id}</strong>
      <p>{primaryProjectItem.recommended}</p>
      <small>{primaryProjectItem.workerDetail}</small>
    {:else}
      <strong>Idle</strong>
      <p>No selected issue is visible for this lane.</p>
      <small>Refresh overview for the latest Project and worker readback.</small>
    {/if}
  </article>

  <article class="target-chart">
    <span class="mini-label">Project states</span>
    {#each projectStateRows as [target, count]}
      <div>
        <span>{String(target)}</span>
        <meter min="0" max={projectItems.length || 1} value={Number(count)}>{Number(count)}</meter>
        <strong>{Number(count)}</strong>
      </div>
    {/each}
  </article>

  <article class="evidence-rail">
    <span class="mini-label">{evidenceRail.length ? 'Evidence trail' : 'Project queue'}</span>
    {#if evidenceRail.length}
      {#each evidenceRail as worker}
        <div>
          <span>{worker.issue}</span>
          <p>{worker.evidence}</p>
        </div>
      {/each}
    {:else if projectItems.length}
      {#each projectItems.slice(0, 4) as item}
        <div>
          <span>{item.id}</span>
          <p>{item.state} · {item.workerStatus}</p>
        </div>
      {/each}
    {:else}
      <div>
        <span>Idle</span>
        <p>No Project queue or worker evidence is visible for this lane.</p>
      </div>
    {/if}
  </article>
</section>

<section class="lane-match-panel {matchTone}" aria-label={`${title} Project and worker match`}>
  <div>
    <span class="mini-label">Project / worker match</span>
    <h3>{matchLabel}</h3>
  </div>
  <p>{matchDetail} {projectItems.length} Project item{projectItems.length === 1 ? '' : 's'} · {workers.length} current worker{workers.length === 1 ? '' : 's'}</p>
</section>

<section class="lane-project-grid" aria-label={`${title} Project queue`}>
  {#if projectItems.length}
    {#each projectItems as item}
      <article class="lane-project-card {item.tone}">
        <div>
          <span class="issue-tag">{item.id}</span>
          <strong>{item.title}</strong>
        </div>
        <p>{item.recommended}</p>
        <div class="queue-card-meta">
          <span>{item.state} · {item.source}</span>
          <strong class="{item.workerTone}">{item.workerStatus}</strong>
        </div>
        <small>{item.workerDetail}</small>
        {#if item.url}
          <a class="queue-link" href={item.url} target="_blank" rel="noreferrer">Open issue</a>
        {/if}
      </article>
    {/each}
  {:else}
    <div class="inline-empty">
      <strong>No Project items in this lane</strong>
      <p>The live queue scan did not surface Todo, Rework, Agent Review, Human Review, or Merging work for this lane.</p>
    </div>
  {/if}
</section>

<section class="worker-grid" aria-label={`${title} workers`}>
  {#if visibleWorkers.length}
    {#each visibleWorkers as worker}
      <article class="worker-card">
        <div class="worker-card-head">
          <div>
            <span class="issue-tag">{worker.issue}</span>
            <h3>{worker.title}</h3>
          </div>
          <span class="status-pill neutral">{worker.elapsed}</span>
        </div>

        <dl class="worker-meta">
          <div>
            <dt>Action</dt>
            <dd>{worker.action}</dd>
          </div>
          <div>
            <dt>Backend / session</dt>
            <dd>{worker.backend} · {worker.session}</dd>
          </div>
          <div>
            <dt>Latest evidence</dt>
            <dd>{worker.evidence}</dd>
          </div>
          <div>
            <dt>Target transition</dt>
            <dd>{worker.target}</dd>
          </div>
        </dl>
      </article>
    {/each}
  {:else}
    <div class="inline-empty">
      <strong>No current worker visible</strong>
      <p>Project work may still be queued. Start or resume the lane through chat Skills when you choose to act.</p>
    </div>
  {/if}
</section>
