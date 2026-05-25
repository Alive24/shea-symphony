<script lang="ts">
  export let brief = null;

  $: focus = brief?.focus;
  $: lanes = brief?.lanes ?? [];
  $: skills = brief?.skills ?? [];
  $: evidence = brief?.evidence ?? [];
</script>

<section class="operator-brief" aria-labelledby="operator-brief-title">
  <div class="section-heading">
    <div>
      <p class="eyebrow">Operator Brief</p>
      <h2 id="operator-brief-title">Today&apos;s Visual Readback</h2>
    </div>
    <span class="section-note">{brief?.sourceNote ?? brief?.trust ?? 'Waiting for local API'}</span>
  </div>

  <div class="brief-grid">
    <article class="brief-focus {focus?.tone ?? 'neutral'}">
      <span class="mini-label">Highest attention</span>
      <div>
        <strong>{focus?.id ?? 'None'}</strong>
        <h3>{focus?.title ?? 'No parked operator item is visible'}</h3>
      </div>
      <p>{focus?.reason ?? 'Refresh live data to confirm whether Shea Symphony has work needing a human decision.'}</p>
      <small>{focus?.recommended ?? 'Use chat Skills for routing after a live readback.'}</small>
    </article>

    <article class="brief-panel">
      <span class="mini-label">Skill route</span>
      <div class="brief-chip-grid">
        {#each skills as skill}
          <div class="brief-chip {skill.tone}">
            <strong>{skill.count}</strong>
            <span>{skill.label}</span>
          </div>
        {/each}
      </div>
    </article>

    <article class="brief-panel">
      <span class="mini-label">Lane pressure</span>
      <div class="brief-lanes">
        {#each lanes as lane}
          <div>
            <span>
              {lane.name}
              <small>{lane.sourceLabel}</small>
            </span>
            <meter min="0" max={brief?.laneMax ?? 1} value={lane.pressure}>{lane.pressure}</meter>
            <strong>{lane.pressure}</strong>
          </div>
        {/each}
      </div>
    </article>

    <article class="brief-panel">
      <span class="mini-label">Evidence coverage</span>
      <div class="brief-evidence">
        {#each evidence as item}
          <div>
            <strong>{item.lane}</strong>
            <span>{item.count} signal{item.count === 1 ? '' : 's'}</span>
          </div>
        {/each}
      </div>
    </article>
  </div>
</section>
