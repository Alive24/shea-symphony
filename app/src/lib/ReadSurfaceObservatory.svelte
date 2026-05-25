<script lang="ts">
  export let commands = [];

  $: passed = commands.filter((command) => command.status === 'Passed');
  $: failed = commands.filter((command) => command.status === 'Failed');
  $: pending = commands.filter((command) => command.status === 'Pending');
  $: timedOut = commands.filter((command) => command.exit?.startsWith('timeout'));
</script>

<section class="read-observatory" aria-labelledby="read-observatory-title">
  <div class="section-heading">
    <div>
      <p class="eyebrow">Read Surface Observatory</p>
      <h2 id="read-observatory-title">What Can We Trust Right Now?</h2>
    </div>
    <span class="section-note">
      {passed.length} usable · {pending.length} pending · {timedOut.length} slow read{timedOut.length === 1 ? '' : 's'} · {failed.length} degraded
    </span>
  </div>

  <div class="read-observatory-grid">
    {#each commands as command}
      <article class="read-surface-card {command.tone}">
        <div>
          <strong>{command.label}</strong>
          <span class="status-pill {command.tone}">{command.status}</span>
        </div>
        <p>{command.impact}</p>
        <small>{command.recommendation}</small>
      </article>
    {/each}
  </div>
</section>
