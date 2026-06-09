<script lang="ts">
  import BrandRefreshStatus from './BrandRefreshStatus.svelte';
  import type { NavigatorNavigateHandler } from './NavigatorTypes.ts';

  export let refreshRunning = false;
  export let refreshRemaining = 0;
  export let refreshFinishedAt: string | null = null;
  export let onNavigate: NavigatorNavigateHandler = () => {};
</script>

<a class:loading={refreshRunning} class="brand-lockup" href="/" onclick={(event) => onNavigate(event, '/')}>
  <span class="brand-mark" aria-hidden="true">
    <span>SS</span>
    <span class="brand-loader"></span>
  </span>
  <span>
    <strong>Shea Symphony</strong>
    <BrandRefreshStatus
      running={refreshRunning}
      remaining={refreshRemaining}
      finishedAt={refreshFinishedAt}
    />
  </span>
</a>
