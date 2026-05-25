<script lang="ts">
  import { onMount } from 'svelte';
  import LaneDetail from '$lib/LaneDetail.svelte';
  import { DATA_MODE_CHANGE_EVENT, buildViewModel, loadOverview } from '$lib/api';

  let workers = [];
  let projectItems = [];
  let generatedAtLabel = 'not checked';
  let backgroundRefreshing = false;
  const autoRefreshMs = 45_000;

  async function refresh(background = false) {
    backgroundRefreshing = background;
    try {
      const view = buildViewModel(await loadOverview(false, 'fast'));
      workers = view.laneWorkers?.merge ?? [];
      projectItems = view.laneProjectIssues?.merge ?? [];
      generatedAtLabel = view.generatedAtLabel;
    } catch (_) {
      workers = [];
      projectItems = [];
    } finally {
      backgroundRefreshing = false;
    }
  }

  onMount(() => {
    refresh();
    const autoRefresh = window.setInterval(() => {
      if (backgroundRefreshing || document.visibilityState !== 'visible') return;
      refresh(true);
    }, autoRefreshMs);
    const dataModeListener = () => refresh();
    window.addEventListener(DATA_MODE_CHANGE_EVENT, dataModeListener);
    return () => {
      window.removeEventListener(DATA_MODE_CHANGE_EVENT, dataModeListener);
      window.clearInterval(autoRefresh);
    };
  });
</script>

<LaneDetail
  title="Merge lane"
  description="Approved work lands here. Clean merges stay mechanical; unsafe conflicts are routed back to a human instead of blurring Human Review."
  {workers}
  {projectItems}
  {generatedAtLabel}
  {backgroundRefreshing}
/>
