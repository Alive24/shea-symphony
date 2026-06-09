<script lang="ts">
  export let lanes: any[] = [];
  export let refreshing = false;
  export let fullLoading = false;
  export let hasStableLanes = false;
  export let autoloopRunning = false;
  export let tauriAvailable = false;
  export let autoloopMode = 'dry-run';
  export let workflowPath = 'workflows/shea-symphony.md';
  export let latestAutoloopLine = '';
  export let slowReadsRemaining = 0;

  $: loadingInitial = fullLoading && !hasStableLanes;
</script>

<section
  class:refreshing
  class="lane-board-overview"
  aria-label="Worker pickup and queue by lane"
  aria-busy={refreshing}
>
  <div class="lane-board-grid">
    {#each lanes as lane}
      <article class="lane-board-column {lane.tone}">
        <div class="lane-board-column-head compact">
          <strong>{lane.label}</strong>
          <span
            class="lane-board-state-slot {lane.refreshing || loadingInitial ? 'loading' : lane.status}"
            aria-label={lane.refreshing || loadingInitial
              ? `${lane.label} loading`
              : lane.status === 'complete'
              ? `${lane.label} complete`
              : `${lane.label} ${lane.status}`}
          >
            {#if lane.refreshing || loadingInitial}
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
            <div class="lane-board-empty">{fullLoading && !lane.refreshing && !hasStableLanes ? 'Loading CLI readback...' : 'No issue visible.'}</div>
          {/if}
        </div>
      </article>
    {/each}
  </div>

  <div class="autoloop-control-bar" aria-label="Autoloop controls">
    <div>
      <strong>{autoloopRunning ? 'Autoloop running' : 'Autoloop idle'}</strong>
      <span>
        {tauriAvailable ? `${autoloopMode} · ${workflowPath}` : 'Open in Shea Symphony App desktop shell for live loop control.'}
      </span>
      {#if latestAutoloopLine}
        <small>{latestAutoloopLine}</small>
      {:else if fullLoading}
        <small>Loading CLI readback · {slowReadsRemaining} surface{slowReadsRemaining === 1 ? '' : 's'} remaining</small>
      {/if}
    </div>
  </div>
</section>
