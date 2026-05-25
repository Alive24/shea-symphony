<script lang="ts">
  export let commands = [];

  $: passed = commands.filter((command) => command.status === 'Passed').length;
  $: failed = commands.filter((command) => command.status === 'Failed').length;
</script>

<section class="command-health" aria-labelledby="command-health-title">
  <div class="section-heading">
    <div>
      <p class="eyebrow">Overview Commands</p>
      <h2 id="command-health-title">Read Surface Matrix</h2>
    </div>
    <span class="section-note">{passed} passed · {failed} failed · {commands.length} total</span>
  </div>

  {#if commands.length}
    <div class="command-health-grid">
      {#each commands as command}
        <article class="command-health-card {command.tone}">
          <div>
            <strong>{command.label}</strong>
            <span class="status-pill {command.tone}">{command.status}</span>
          </div>
          <dl>
            <div>
              <dt>Duration</dt>
              <dd>{command.duration}</dd>
            </div>
            <div>
              <dt>Exit</dt>
              <dd>{command.exit}</dd>
            </div>
            <div>
              <dt>Command</dt>
              <dd>{command.args}</dd>
            </div>
            <div>
              <dt>Signal</dt>
              <dd>{command.detail}</dd>
            </div>
          </dl>
        </article>
      {/each}
    </div>
  {:else}
    <div class="inline-empty">
      <strong>No read surface checks captured</strong>
      <p>The matrix will show Autopilot, Doctor, Review, Skills, and Session reads after the Tauri bridge returns command evidence.</p>
    </div>
  {/if}
</section>
