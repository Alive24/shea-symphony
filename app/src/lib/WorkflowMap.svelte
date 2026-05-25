<script lang="ts">
  export let visualPosture = [];
  export let stateTiles = [];
  export let stateDistribution = [];
  export let stateMax = 1;
  export let fixture = false;
</script>

<section class="visual-dashboard" aria-labelledby="visual-dashboard-title">
  <div class="section-heading">
    <div>
      <p class="eyebrow">Visualization</p>
      <h2 id="visual-dashboard-title">Workflow Map</h2>
    </div>
    <span class="section-note">{fixture ? 'Fixture evidence' : 'Live evidence when available'}</span>
  </div>

  <div class="visual-grid">
    <section class="flow-panel" aria-label="Shea Symphony state flow">
      <div class="flow-track">
        {#each visualPosture as stage, index}
          <article class="flow-node {stage.status}" class:terminal={index === visualPosture.length - 1}>
            <span>{stage.role}</span>
            <strong>{stage.name}</strong>
            <p>{stage.detail}</p>
            <small>
              {stage.lane
                ? `${stage.lane.active} active · ${stage.lane.blocked} blocked · ${stage.lane.sourceLabel ?? 'Unknown source'}`
                : stage.status}
            </small>
          </article>
        {/each}
      </div>
    </section>

    <aside class="state-panel" aria-label="Current visual counters">
      <div class="state-tile-grid">
        {#each stateTiles as tile}
          <article class="state-tile {tile.tone}">
            <strong>{tile.value}</strong>
            <span>{tile.label}</span>
          </article>
        {/each}
      </div>

      <div class="evidence-board">
        <div>
          <p class="eyebrow">State Distribution</p>
          <h3>Visible Queues</h3>
        </div>
        {#each stateDistribution.slice(0, 6) as row}
          <div class="distribution-row {row.tone}">
            <span>
              {row.state}
              <small>{row.sourceLabel}</small>
            </span>
            <meter min="0" max={stateMax} value={row.count}>{row.count}</meter>
            <strong>{row.count}</strong>
          </div>
        {/each}
      </div>
    </aside>
  </div>
</section>
