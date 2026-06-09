<script module>
  import { defineMeta } from '@storybook/addon-svelte-csf';
  import OperatorDeskStoryHarness from './OperatorDeskStoryHarness.svelte';
  import { buildFixtureOverview } from './lib/operatorFixtures.ts';
  import { buildViewModel } from './lib/operatorViewModel.ts';

  function clone(value) {
    return JSON.parse(JSON.stringify(value));
  }

  function minutesAgo(minutes) {
    return new Date(Date.now() - minutes * 60 * 1000).toISOString();
  }

  function deskView(mutator = null) {
    const overview = clone(buildFixtureOverview(true));
    mutator?.(overview);
    return buildViewModel(overview);
  }

  const activeDeskView = deskView((overview) => {
    overview.sessionsText = [
      'agent_session lane=main issue=#418 title="Forge contract needs blocker relationship clarification" backend="Codex app-server" session=desk-main pid=8123 status=running target="Need to Clarify"',
      'agent_session lane=review issue=#421 title="Agent Review evidence needs Human Review routing" backend="Gemini CLI" session=desk-review pid=8124 status=running target="Human Review"',
      'agent_session lane=merge issue=#430 title="Merge lane should land approved app-server cleanup" backend="Codex app-server" session=desk-merge pid=8125 status=running target=Done'
    ].join('\n');
  });

  const refreshingDeskView = deskView((overview) => {
    overview.githubQueue.issues = overview.githubQueue.issues.map((issue) => ({
      ...issue,
      updatedAt: minutesAgo(4)
    }));
  });

  const emptyDeskView = deskView((overview) => {
    overview.githubQueue.issues = [];
    overview.githubQueue.operatorIssues = [];
    overview.githubQueue.totalOpen = 0;
    overview.githubQueue.stateCounts = {};
    overview.githubQueue.laneCounts = { main: 0, review: 0, merge: 0 };
    overview.autopilot.lanes = [
      { lane: 'main', status: 'idle', reason: 'No issue selected.' },
      { lane: 'review', status: 'idle', reason: 'No issue selected.' },
      { lane: 'merge', status: 'idle', reason: 'No issue selected.' }
    ];
    overview.autopilot.parked_queues = [];
    overview.sessionsText = 'agent_session_list=none';
  });

  const { Story } = defineMeta({
    title: 'Pages/OperatorDesk',
    component: OperatorDeskStoryHarness,
    tags: ['autodocs'],
    parameters: {
      layout: 'fullscreen'
    },
    args: {
      view: activeDeskView,
      fullLoading: false,
      slowReadsRemaining: 0,
      refreshing: false
    }
  });
</script>

{#snippet template(args)}
  <main class="operator-desk-story-shell" aria-label="OperatorDesk Storybook preview">
    <OperatorDeskStoryHarness {...args} />
  </main>
{/snippet}

<Story name="Active desk" {template} />
<Story
  name="Refreshing desk"
  args={{
    view: refreshingDeskView,
    refreshing: true,
    slowReadsRemaining: 2
  }}
  {template}
/>
<Story
  name="Empty desk"
  args={{
    view: emptyDeskView
  }}
  {template}
/>

<style>
  .operator-desk-story-shell {
    width: 1920px;
    height: 1080px;
    padding: var(--space-6);
    color: var(--fg);
    background: var(--bg);
    overflow: hidden;
  }

  .operator-desk-story-shell :global(.operator-first-screen) {
    height: calc(1080px - 148px);
    min-height: 560px;
    margin-bottom: 0;
  }

  .operator-desk-story-shell :global(.human-todo-rail) {
    grid-auto-columns: 382px;
  }

  .operator-desk-story-shell :global(.human-todo-card),
  .operator-desk-story-shell :global(.human-todo-empty) {
    height: 100%;
  }

  .operator-desk-story-shell :global(.lane-board-overview) {
    height: 100%;
  }

  @media (max-width: 720px) {
    .operator-desk-story-shell {
      width: 100%;
      height: auto;
      min-height: 100vh;
      padding: var(--space-4);
      overflow: visible;
    }

    .operator-desk-story-shell :global(.operator-first-screen) {
      height: auto;
    }

    .operator-desk-story-shell :global(.human-todo-rail) {
      grid-auto-columns: minmax(260px, 84vw);
    }
  }
</style>
