<script module>
  import { defineMeta } from '@storybook/addon-svelte-csf';
  import LaneIssueView from './LaneIssueView.svelte';
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

  function minutesAgo(minutes) {
    return new Date(Date.now() - minutes * 60 * 1000).toISOString();
  }

  const transcriptFixture = {
    status: 'available',
    deepLink: 'codex://threads/019e8f37-5cab-74f3-9933-93e3809396e5',
    path: '/tmp/shea-symphony/logs/app-server/issue-409.protocol.jsonl',
    threadId: '019e8f37-5cab-74f3-9933-93e3809396e5',
    lastUserMessageAt: new Date(Date.now() - 16 * 60 * 1000).toISOString(),
    lastAssistantMessageAt: new Date(Date.now() - 12 * 60 * 1000).toISOString()
  };
  const completedIssue = {
    ...issueView('409'),
    transcriptFixture
  };
  const normalIssue = {
    ...issueView('430', (overview) => {
      overview.sessionsText = 'agent_session lane=merge issue=#430 title="Merge lane should land approved app-server cleanup" backend="Codex app-server" session=merge-430 pid=8129 status=running target=Done';
      overview.localStatus.issueLifecycle['#430'] = [
        {
          phase: 'Backlog',
          label: 'Issue visible in tracker',
          time: minutesAgo(90),
          detail: 'Tracker issue readback is available.',
          url: 'https://github.com/Alive24/shea-symphony/issues/430'
        },
        {
          phase: 'Promoted',
          label: 'Promoted into Merge',
          time: minutesAgo(22),
          detail: 'Merging',
          url: 'https://github.com/Alive24/shea-symphony/issues/430#issuecomment-1004301'
        }
      ];
      overview.autopilot.active_issues = [
        {
          lane: 'merge',
          issue: '#430',
          backend: 'Codex app-server',
          session: 'merge-430',
          pid: 8129
        }
      ];
    }),
    transcriptFixture: {
      ...transcriptFixture,
      path: '/tmp/shea-symphony/logs/app-server/issue-430.protocol.jsonl'
    }
  };
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
    component: LaneIssueView,
    tags: ['autodocs'],
    parameters: {
      layout: 'fullscreen'
    },
    args: completedIssue
  });
</script>

{#snippet template(args)}
  <main class="lane-issue-view-story-shell" aria-label="LaneIssueView Storybook preview">
    <LaneIssueView {...args} />
  </main>
{/snippet}

<Story name="Completed issue" {template} />
<Story name="Normal issue" args={normalIssue} {template} />
<Story name="Human review issue" args={humanReviewIssue} {template} />
<Story name="Active runtime issue" args={activeRuntimeIssue} {template} />

<style>
  .lane-issue-view-story-shell {
    width: 100%;
    max-width: 1920px;
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
