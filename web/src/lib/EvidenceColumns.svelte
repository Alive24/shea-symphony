<script lang="ts">
  export let title = 'Lane Signals';
  export let eyebrow = 'Evidence Flow';
  export let columns = [];
  export let href = '';

  $: eventCount = columns.reduce((count, column) => count + (column.events?.length ?? 0), 0);
</script>

<section class="evidence-dashboard" aria-labelledby="evidence-dashboard-title">
  <div class="section-heading">
    <div>
      <p class="eyebrow">{eyebrow}</p>
      <h2 id="evidence-dashboard-title">{title}</h2>
    </div>
    {#if href}
      <a class="btn btn-ghost" href={href}>Open event log</a>
    {:else}
      <span class="section-note">{eventCount} event{eventCount === 1 ? '' : 's'}</span>
    {/if}
  </div>

  {#if columns.length}
    <div class="evidence-columns">
      {#each columns as column}
        <article class="evidence-column">
          <strong>{column.lane}</strong>
          {#each column.events as event}
            <div>
              <span>{event.time}</span>
              <p>{event.title}</p>
              <small>{event.detail}</small>
            </div>
          {/each}
        </article>
      {/each}
    </div>
  {:else}
    <div class="inline-empty">
      <strong>No lane evidence visible yet</strong>
      <p>Evidence columns fill from overview events. Until then, use the runbook page to pick the right chat Skill before making routing decisions.</p>
    </div>
  {/if}
</section>
