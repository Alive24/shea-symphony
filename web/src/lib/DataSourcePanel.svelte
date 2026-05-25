<script lang="ts">
  export let source;
  export let compact = false;
</script>

{#if source}
  <section class:compact class="data-source-panel {source.tone}" aria-label="Current data source">
    <div>
      <p class="eyebrow">Data Source</p>
      <h3>{source.label}</h3>
    </div>
    <dl>
      <div>
        <dt>Freshness</dt>
        <dd>{source.freshness}</dd>
      </div>
      <div>
        <dt>Trust</dt>
        <dd>{source.trust}</dd>
      </div>
      <div>
        <dt>Detail</dt>
        <dd>{source.detail}</dd>
      </div>
    </dl>
  </section>
{/if}
