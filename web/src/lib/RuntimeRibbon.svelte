<script lang="ts">
  export let source = null;
  export let generatedAtLabel = 'not checked';
  export let healthy = false;
  export let fixture = false;
  export let attentionCount = 0;
  export let diagnosticCount = 0;
  export let blockedCount = 0;

  $: mode = fixture ? 'Fixture' : healthy ? 'Live' : 'Offline';
  $: tone = fixture ? 'warn' : healthy ? 'success' : 'danger';
  $: recommendedTab = attentionCount > 0 ? 'Queue' : blockedCount > 0 ? 'Evidence' : diagnosticCount > 0 ? 'Diagnostics' : 'Overview';
  $: hasPending = source?.detail?.includes('pending slow reads');
  $: posture = fixture
    ? 'Visual QA only'
    : healthy
      ? hasPending
        ? 'Fast readback active'
        : 'Usable for observation'
      : 'Fallback layout data';
</script>

<section class="runtime-ribbon {tone}" aria-label="Runtime observation status">
  <div>
    <span class="mini-label">Runtime</span>
    <strong>{mode}</strong>
    <small>{posture}</small>
  </div>
  <div>
      <span class="mini-label">Data trust</span>
      <strong>{source?.label ?? source?.trust ?? 'Unknown'}</strong>
    <small>{healthy ? 'Live reads available; details in Diagnostics' : source?.trust ?? source?.freshness ?? generatedAtLabel}</small>
  </div>
  <div>
    <span class="mini-label">Start here</span>
    <strong>{recommendedTab}</strong>
    <small>{attentionCount} queue · {blockedCount} blocked · {diagnosticCount} diagnostics</small>
  </div>
</section>
