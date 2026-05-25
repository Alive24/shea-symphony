<script lang="ts">
  export let paths = [];
</script>

<section class="read-path-map" aria-labelledby="read-path-map-title">
  <div class="section-heading">
    <div>
      <p class="eyebrow">Read Path Map</p>
      <h2 id="read-path-map-title">Where Observation Comes From</h2>
    </div>
    <span class="section-note">Web -> CLI -> Shea surfaces</span>
  </div>

  <div class="read-path-track">
    {#each paths as path, index}
      <article class="read-path-node {path.tone}" class:terminal={index === paths.length - 1}>
        <span>{path.role}</span>
        <strong>{path.label}</strong>
        <p>{path.detail}</p>
        <small>{path.status} · {path.exit}</small>
      </article>
    {/each}
  </div>
</section>
