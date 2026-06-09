<script module>
  import { defineMeta } from '@storybook/addon-svelte-csf';
  import LaneViews from './LaneViews.svelte';
  import { buildFixtureOverview } from './operatorFixtures.ts';
  import { buildViewModel } from './operatorViewModel.ts';

  function clone(value) {
    return JSON.parse(JSON.stringify(value));
  }

  function issueView(routeIssue, mutator = null) {
    const overview = clone(buildFixtureOverview(true));
    mutator?.(overview);
    return {
      route: `/lanes/${routeIssue}`,
      view: buildViewModel(overview)
    };
  }

  const completedIssue = issueView('409');
  const humanReviewIssue = issueView('425', (overview) => {
    overview.localStatus.issueLifecycle['#425'] = [
      {
        phase: 'Backlog',
        label: 'Issue visible in tracker',
        time: overview.generatedAt,
        url: 'https://github.com/Alive24/shea-symphony/issues/425'
      },
      {
        phase: 'Human Review',
        label: 'Awaiting operator approval',
        time: overview.generatedAt,
        detail: 'Storybook fixture keeps this issue in Human Review.',
        url: 'https://github.com/Alive24/shea-symphony/issues/425#issuecomment-1004251'
      }
    ];
  });

  const activeRuntimeIssue = issueView('418', (overview) => {
    overview.sessionsText = 'agent_session lane=main issue=#418 title="Forge contract needs blocker relationship clarification" backend="Codex app-server" session=lane-issue-main pid=8123 status=running target="Need to Clarify"';
  });

  const { Story } = defineMeta({
    title: 'Pages/LaneIssueView',
    component: LaneViews,
    tags: ['autodocs'],
    parameters: {
      layout: 'fullscreen'
    },
    args: completedIssue
  });
</script>

{#snippet template(args)}
  <main class="lane-issue-view-story-shell" aria-label="Lane issue detail Storybook preview">
    <LaneViews {...args} />
  </main>
{/snippet}

<Story name="Completed issue" {template} />
<Story name="Human review issue" args={humanReviewIssue} {template} />
<Story name="Active runtime issue" args={activeRuntimeIssue} {template} />

<style>
  .lane-issue-view-story-shell {
    width: 1920px;
    min-height: 1080px;
    padding: var(--space-6);
    color: var(--fg);
    background: var(--bg);
  }

  @media (max-width: 720px) {
    .lane-issue-view-story-shell {
      width: 100%;
      min-height: 100vh;
      padding: var(--space-4);
    }
  }
</style>
