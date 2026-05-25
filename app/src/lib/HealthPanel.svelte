<script lang="ts">
  export let health = null;
  export let error = '';
</script>

<section class="health-panel" aria-label="Local web health">
  <div>
    <p class="eyebrow">Local Web Health</p>
    <h3>{error ? 'Health unavailable' : health?.fixture ? 'Fixture server ready' : 'Server ready'}</h3>
  </div>

  {#if error}
    <p>{error}</p>
  {:else if health}
    <div class="health-grid">
      <div>
        <span>Build</span>
        <strong>{health.buildPresent ? 'Present' : 'Missing'}</strong>
      </div>
      <div>
        <span>CLI mode</span>
        <strong>{health.cli?.mode}</strong>
      </div>
      <div>
        <span>Workflow</span>
        <strong>{health.workflowPath}</strong>
      </div>
      <div>
        <span>Bind</span>
        <strong>{health.server?.host}:{health.server?.port}</strong>
      </div>
      <div>
        <span>Overview timeout</span>
        <strong>{Math.round((health.server?.overviewTimeoutMs ?? 0) / 1000)}s</strong>
      </div>
    </div>
  {:else}
    <p>Waiting for local server health.</p>
  {/if}
</section>
