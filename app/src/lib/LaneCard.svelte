<script lang="ts">
  export let lane;
</script>

<article class="lane-card {lane.posture}">
  <div class="lane-card-head">
    <div>
      <div class="label-row">
        <span class="mini-label">{lane.posture}</span>
        <span class="source-pill {lane.sourceTone ?? 'warn'}">{lane.sourceLabel ?? 'Unknown source'}</span>
      </div>
      <h3>{lane.name}</h3>
    </div>
    <a class="btn btn-ghost" href={lane.href}>View lane</a>
  </div>

  <div class="lane-metrics" aria-label={`${lane.name} worker summary`}>
    <div>
      <strong>{lane.active}</strong>
      <span>active</span>
    </div>
    <div>
      <strong>{lane.retrying}</strong>
      <span>retrying</span>
    </div>
    <div>
      <strong>{lane.blocked}</strong>
      <span>blocked</span>
    </div>
  </div>

  <p>{lane.latest}</p>
</article>
