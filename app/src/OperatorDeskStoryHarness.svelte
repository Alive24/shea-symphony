<script lang="ts">
  import { onMount } from 'svelte';
  import OperatorDesk from './OperatorDesk.svelte';
  import { operatorOverviewStore } from './lib/operatorOverviewStore.ts';
  import { refreshStatusStore } from './lib/uiState.ts';

  export let view: any;
  export let fullLoading = false;
  export let slowReadsRemaining = 0;
  export let refreshing = false;

  const idleLocalArtifactsRefresh = {
    running: false,
    remaining: 0,
    startedAt: null,
    lastRefreshedAt: null,
    error: '',
    source: 'storybook'
  };

  $: operatorOverviewStore.set({
    view,
    loading: false,
    fullLoading,
    backgroundRefreshing: refreshing,
    slowReadsRemaining,
    liveError: '',
    localArtifactsRefresh: idleLocalArtifactsRefresh,
    projectReadCooldown: null
  });

  $: refreshStatusStore.set({
    running: refreshing,
    remaining: refreshing ? slowReadsRemaining : 0,
    startedAt: refreshing ? new Date().toISOString() : null,
    finishedAt: refreshing ? null : new Date().toISOString(),
    source: 'storybook',
    detail: refreshing ? 'Refreshing Storybook fixture' : 'Idle'
  });

  onMount(() => {
    localStorage.setItem('shea-handoff-target', 'codex-app');
  });
</script>

<OperatorDesk />
