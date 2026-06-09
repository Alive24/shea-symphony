<script module>
  import { defineMeta } from '@storybook/addon-svelte-csf';
  import LaneBoard from './LaneBoard.svelte';

  const activeLaneBoard = [
    {
      laneKey: 'main',
      label: 'Main',
      tone: 'success',
      status: 'complete',
      refreshing: false,
      issues: [
        {
          kind: 'picked',
          id: '#442',
          title: 'Stale claim recovery should release preserved worktrees',
          meta: 'Implementing · Codex app-server · session pending',
          tone: 'success',
          workerNumber: 1,
          waiting: true
        },
        {
          kind: 'queued',
          id: '#445',
          title: 'Forge validation should report assignee flag drift',
          meta: 'Todo · Run Issue Quality Gate before dispatch.',
          tone: 'warn',
          workerNumber: null,
          waiting: false
        }
      ]
    },
    {
      laneKey: 'review',
      label: 'Review',
      tone: 'warn',
      status: 'blocked',
      refreshing: false,
      issues: [
        {
          kind: 'blocked',
          id: 'Blocked',
          title: 'Lane blocked',
          meta: 'Need Human Input · blocked',
          tone: 'danger',
          workerNumber: null,
          waiting: false
        }
      ]
    },
    {
      laneKey: 'merge',
      label: 'Merge',
      tone: 'neutral',
      status: 'idle',
      refreshing: false,
      issues: []
    }
  ];

  const refreshingLaneBoard = activeLaneBoard.map((lane) => ({ ...lane, refreshing: true }));
  const emptyLaneBoard = activeLaneBoard.map((lane) => ({
    ...lane,
    tone: 'neutral',
    status: 'idle',
    refreshing: false,
    issues: []
  }));

  const { Story } = defineMeta({
    title: 'Operator Components/LaneBoard',
    component: LaneBoard,
    tags: ['autodocs'],
    parameters: {
      layout: 'fullscreen'
    },
    args: {
      lanes: activeLaneBoard,
      refreshing: false,
      fullLoading: false,
      hasStableLanes: true,
      autoloopRunning: true,
      tauriAvailable: true,
      autoloopMode: 'live',
      workflowPath: 'workflows/shea-symphony.md',
      latestAutoloopLine: 'main picked #442; review waiting on human input',
      slowReadsRemaining: 0
    }
  });
</script>

{#snippet template(args)}
  <main class="lane-board-story-shell" aria-label="Lane board preview">
    <section class="lane-board-story-frame">
      <LaneBoard {...args} />
    </section>
  </main>
{/snippet}

<Story name="Active board" {template} />
<Story
  name="Refreshing board"
  args={{
    lanes: refreshingLaneBoard,
    refreshing: true,
    latestAutoloopLine: 'Refreshing local artifacts after autoloop lane update'
  }}
  {template}
/>
<Story
  name="Initial loading"
  args={{
    lanes: emptyLaneBoard,
    fullLoading: true,
    hasStableLanes: false,
    autoloopRunning: false,
    tauriAvailable: false,
    latestAutoloopLine: '',
    slowReadsRemaining: 3
  }}
  {template}
/>
<Story
  name="Idle empty"
  args={{
    lanes: emptyLaneBoard,
    autoloopRunning: false,
    tauriAvailable: true,
    autoloopMode: 'dry-run',
    latestAutoloopLine: 'No recent autoloop result'
  }}
  {template}
/>

<style>
  .lane-board-story-shell {
    width: 1920px;
    min-height: 420px;
    padding: var(--space-6);
    color: var(--fg);
    background: var(--bg);
  }

  .lane-board-story-frame {
    width: 1560px;
    height: 352px;
    margin: 0 auto;
  }

  .lane-board-story-frame :global(.lane-board-overview) {
    height: 100%;
  }

  @media (max-width: 720px) {
    .lane-board-story-shell {
      width: 100%;
      height: auto;
      min-height: 100vh;
      padding: var(--space-4);
    }

    .lane-board-story-frame {
      width: 100%;
      height: auto;
    }
  }
</style>
