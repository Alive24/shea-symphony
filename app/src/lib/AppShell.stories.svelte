<script module>
  import { defineMeta } from '@storybook/addon-svelte-csf';
  import AppShell from './AppShell.svelte';
  import LaneBoard from './LaneBoard.svelte';

  const emptyLaneBoard = ['Main', 'Review', 'Merge'].map((label) => ({
    laneKey: label.toLowerCase(),
    label,
    tone: 'neutral',
    status: 'idle',
    refreshing: false,
    issues: []
  }));

  const { Story } = defineMeta({
    title: 'Components/AppShell',
    component: AppShell,
    tags: ['autodocs'],
    parameters: {
      layout: 'fullscreen'
    },
    args: {
      currentPath: '/'
    }
  });
</script>

<script>
  import { onMount } from 'svelte';

  let ready = false;

  onMount(() => {
    localStorage.setItem('shea-developer-tools-open', 'true');
    localStorage.setItem('shea-developer-tools-collapsed', 'true');
    ready = true;
  });
</script>

{#snippet template(args)}
  <main class="app-shell-story-shell" aria-label="Storybook 1920 by 1080 app shell preview">
    {#if ready}
      <AppShell {...args}>
        <section class="app-shell-story-first-screen" aria-label="Operator first screen layout preview">
          <section class="app-shell-story-human-strip" aria-hidden="true">
            <article></article>
            <article></article>
          </section>
          <LaneBoard
            lanes={emptyLaneBoard}
            refreshing={false}
            fullLoading={false}
            hasStableLanes={true}
            autoloopRunning={false}
            tauriAvailable={true}
            autoloopMode="dry-run"
            workflowPath=".shea/workflows/shea-symphony.md"
            latestAutoloopLine="No recent autoloop result"
            slowReadsRemaining={0}
          />
        </section>
      </AppShell>
    {/if}
  </main>
{/snippet}

<Story name="Operator desk shell" {template} />

<style>
  .app-shell-story-shell {
    width: 1920px;
    height: 1080px;
    color: var(--fg);
    background: var(--bg);
    overflow: hidden;
  }

  .app-shell-story-shell :global(.app-chrome) {
    min-height: 1080px;
  }

  .app-shell-story-first-screen {
    height: calc(1080px - 148px);
    min-height: 560px;
    display: grid;
    grid-template-rows: minmax(0, 61.8fr) minmax(0, 38.2fr);
    gap: var(--space-3);
  }

  .app-shell-story-human-strip {
    min-height: 0;
    display: flex;
    align-items: stretch;
    gap: var(--space-3);
  }

  .app-shell-story-human-strip article {
    width: 382px;
    border: 3px solid var(--project-pink);
    border-radius: var(--radius-lg);
    background: var(--card-bg-strong);
  }
</style>
