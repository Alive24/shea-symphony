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
      workers = view.laneWorkers?.review ?? [];
      projectItems = view.laneProjectIssues?.review ?? [];
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
  title="Review lane"
  description="Independent review stays separate from Main implementation. Passing evidence can brief Human Review, while confirmed findings route to Rework."
  {workers}
  {projectItems}
  {generatedAtLabel}
  {backgroundRefreshing}
/>
