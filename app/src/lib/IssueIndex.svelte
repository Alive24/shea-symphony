<script lang="ts">
  export let issues = [];
  export let limit = 8;

  $: visibleIssues = issues.slice(0, limit);
  $: dangerCount = issues.filter((issue) => issue.tone === 'danger').length;
  $: laneCount = new Set(issues.map((issue) => issue.lane).filter(Boolean)).size;
</script>

<section class="issue-index" aria-labelledby="issue-index-title">
  <div class="section-heading">
    <div>
      <p class="eyebrow">Issue Index</p>
      <h2 id="issue-index-title">Cross-Lane Evidence</h2>
    </div>
    <span class="section-note">
      {issues.length} issue{issues.length === 1 ? '' : 's'} · {laneCount} lane{laneCount === 1 ? '' : 's'} · {dangerCount} urgent
    </span>
  </div>

  {#if visibleIssues.length}
    <div class="issue-index-grid">
      {#each visibleIssues as issue}
        <article class="issue-index-card {issue.tone}">
          <div class="issue-index-head">
            <span class="issue-tag">{issue.id}</span>
            <span class="status-pill neutral">{issue.lane}</span>
          </div>
          <h3>{issue.title}</h3>
          <dl>
            <div>
              <dt>State / target</dt>
              <dd>{issue.state}</dd>
            </div>
            <div>
              <dt>Recommended</dt>
              <dd>{issue.recommended}</dd>
            </div>
            <div>
              <dt>Evidence</dt>
              <dd>{issue.evidence}</dd>
            </div>
            <div>
              <dt>Sources</dt>
              <dd>{issue.sources}</dd>
            </div>
          </dl>
        </article>
      {/each}
    </div>
  {:else}
    <div class="inline-empty">
      <strong>No cross-lane issues indexed</strong>
      <p>Start the local API or refresh fixture data to populate attention, lane, and event evidence. This empty state is visual only; route issues from chat Skills after a live readback.</p>
    </div>
  {/if}
</section>
