<script lang="ts">
  import { onMount } from 'svelte';
  import EvidenceColumns from '$lib/EvidenceColumns.svelte';
  import { DATA_MODE_CHANGE_EVENT, buildViewModel, loadOverview } from '$lib/api';

  let selectedLane = 'All';
  let openIndex = 0;
  let view = buildViewModel(null);
  let loading = true;
  let backgroundRefreshing = false;
  let lanes: string[] = ['All'];
  const autoRefreshMs = 45_000;

  $: fullEvents = view.fullEvents;
  $: evidenceColumns = view.evidenceColumns ?? [];

  $: lanes = ['All', ...new Set(fullEvents.map((event: any) => String(event.lane)))] as string[];
  $: filteredEvents =
    selectedLane === 'All' ? fullEvents : fullEvents.filter((event) => event.lane === selectedLane);

  async function refresh(background = false) {
    if (background) {
      backgroundRefreshing = true;
    } else {
      loading = true;
    }
    try {
      view = buildViewModel(await loadOverview(false, 'fast'));
    } catch (_) {
      view = buildViewModel(null);
    } finally {
      if (!background) loading = false;
      backgroundRefreshing = false;
    }
  }

  onMount(() => {
    refresh();
    const dataModeListener = () => refresh();
    window.addEventListener(DATA_MODE_CHANGE_EVENT, dataModeListener);
    const autoRefresh = window.setInterval(() => {
      if (loading || backgroundRefreshing || document.visibilityState !== 'visible') return;
      refresh(true);
    }, autoRefreshMs);
    return () => {
      window.removeEventListener(DATA_MODE_CHANGE_EVENT, dataModeListener);
      window.clearInterval(autoRefresh);
    };
  });
</script>

<section class="route-hero">
  <div>
    <p class="eyebrow">Audit trail</p>
    <h2>Events</h2>
    <p>{loading ? 'Refreshing live command evidence.' : backgroundRefreshing ? 'Auto-reading live command evidence.' : `Showing events from ${view.generatedAtLabel} · auto 45s.`}</p>
  </div>
  <div class="segmented" aria-label="Lane filter">
    {#each lanes as lane}
      <button class:active={selectedLane === lane} type="button" on:click={() => (selectedLane = lane)}>
        {lane}
      </button>
    {/each}
  </div>
</section>

<div class="event-overview">
  <EvidenceColumns title="Signals by Lane" eyebrow="Evidence Map" columns={evidenceColumns} />
</div>

<section class="event-stack">
  {#each filteredEvents as event, index}
    <article class="event-card">
      <button type="button" on:click={() => (openIndex = openIndex === index ? -1 : index)}>
        <span>{event.time}</span>
        <strong>{event.title}</strong>
        <em>{event.lane}</em>
      </button>
      {#if openIndex === index}
        <p>{event.detail}</p>
      {/if}
    </article>
  {/each}
</section>
