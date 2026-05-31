<script lang="ts">
  import { onMount } from 'svelte';

  export let running = false;
  export let remaining = 0;
  export let finishedAt: string | null = null;

  let nowMs = Date.now();
  let clock: number | undefined;

  $: label = running
    ? `Refreshing${remaining ? ` (${remaining})` : ''}`
    : finishedAt
      ? `Refreshed ${elapsedLabel(finishedAt, nowMs)} ago`
      : 'Not refreshed';

  function elapsedLabel(value: string, clockMs: number) {
    const elapsedSeconds = Math.max(0, Math.floor((clockMs - new Date(value).getTime()) / 1000));
    if (elapsedSeconds < 60) return `${elapsedSeconds}s`;
    const elapsedMinutes = Math.floor(elapsedSeconds / 60);
    if (elapsedMinutes < 60) return `${elapsedMinutes}m`;
    const elapsedHours = Math.floor(elapsedMinutes / 60);
    return `${elapsedHours}h`;
  }

  onMount(() => {
    clock = window.setInterval(() => {
      nowMs = Date.now();
    }, 1000);
    return () => {
      if (clock) window.clearInterval(clock);
    };
  });
</script>

<small>{label}</small>
