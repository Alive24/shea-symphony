<script module>
  import { defineMeta } from '@storybook/addon-svelte-csf';
  import LaneIssueView from './LaneIssueView.svelte';
  import { buildFixtureOverview } from './operatorFixtures.ts';
  import { buildViewModel } from './operatorViewModel.ts';

  const baseOverview = buildFixtureOverview(true);

  function clone(value) {
    return JSON.parse(JSON.stringify(value));
  }

  function minutesAgo(minutes) {
    return new Date(Date.now() - minutes * 60 * 1000).toISOString();
  }

  function fixtureView(mutator = null) {
    const overview = clone(baseOverview);
    mutator?.(overview);
    return buildViewModel(overview);
  }

  const activeLanesView = fixtureView((overview) => {
    overview.sessionsText = [
      'agent_session lane=main issue=#418 title="Forge contract needs blocker relationship clarification" backend="Codex app-server" session=mp-main pid=8123 status=running target="Need to Clarify"',
      'agent_session lane=merge issue=#430 title="Merge lane should land approved app-server cleanup" backend="Codex app-server" session=mp-merge pid=8129 status=running target=Done'
    ].join('\n');
    overview.autopilot.active_issues = [
      { lane: 'main', issue: '#418', backend: 'Codex app-server', session: 'mp-main', pid: 8123 },
      { lane: 'merge', issue: '#430', backend: 'Codex app-server', session: 'mp-merge', pid: 8129 }
    ];
  });

  const humanQueueView = fixtureView((overview) => {
    overview.githubQueue.issues = [
      ...overview.githubQueue.issues,
      {
        identifier: '#433',
        number: 433,
        title: 'Recovery handoff should ask before tracker mutation',
        state: 'Need Human Input',
        lane: 'Human',
        updatedAt: minutesAgo(12),
        url: 'https://github.com/Alive24/shea-symphony/issues/433',
        labels: ['fixture', 'recovery'],
        assignees: ['operator']
      },
      {
        identifier: '#436',
        number: 436,
        title: 'Issue Forge should preserve discussion-first backlog shaping',
        state: 'Need to Clarify',
        lane: 'Human',
        updatedAt: minutesAgo(27),
        url: 'https://github.com/Alive24/shea-symphony/issues/436',
        labels: ['fixture', 'forge'],
        assignees: ['operator']
      }
    ];
    overview.githubQueue.operatorIssues = overview.githubQueue.issues.filter((issue) =>
      ['Need to Clarify', 'Need Human Input', 'Human Review'].includes(issue.state)
    );
    overview.githubQueue.totalOpen = overview.githubQueue.issues.length;
    overview.autopilot.parked_queues = [
      {
        state: 'Need Human Input',
        reason: 'Operator confirmation required before state mutation.',
        issues: overview.githubQueue.operatorIssues.map((issue) => ({
          identifier: issue.identifier,
          title: issue.title,
          reason: 'Fixture parked task for Storybook lane pressure.',
          evidence: 'Fixture readback: issue is visible in Human Todo.'
        }))
      }
    ];
  });

  const completedWorktreesView = fixtureView((overview) => {
    overview.localStatus.completedIssueWorktrees = [
      ...overview.localStatus.completedIssueWorktrees,
      ...Array.from({ length: 12 }, (_, index) => {
        const number = 460 + index;
        return {
          issue: `#${number}`,
          title: `Completed lane worktree fixture ${number}`,
          state: 'Done',
          lane: index % 2 ? 'Review' : 'Merge',
          url: `https://github.com/Alive24/shea-symphony/issues/${number}`,
          path: `/tmp/shea-symphony/worktrees/issue-${number}-fixture`,
          branch: `feature/issue-${number}-fixture`,
          head: `fixture${number}`,
          completedAt: minutesAgo(20 + index * 31),
          lastProgressAt: minutesAgo(20 + index * 31),
          lastModified: minutesAgo(18 + index * 31),
          treeState: index === 2 ? 'dirty' : 'clean',
          diskBytes: 204800 + index * 4096
        };
      })
    ];
  });

  const emptyLanesView = fixtureView((overview) => {
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
    overview.localStatus.issueWorktrees = [];
    overview.localStatus.completedIssueWorktrees = [];
    overview.localStatus.issueLifecycle = {};
  });

  const { Story } = defineMeta({
    title: 'Pages/Lanes',
    component: LaneIssueView,
    tags: ['autodocs'],
    parameters: {
      layout: 'fullscreen'
    },
    args: {
      view: activeLanesView,
      route: '/lanes'
    }
  });
</script>

{#snippet template(args)}
  <main class="lanes-page-story-shell" aria-label="Lanes page Storybook preview">
    <LaneIssueView {...args} />
  </main>
{/snippet}

<Story name="Active lanes" {template} />
<Story
  name="Human Todo pressure"
  args={{
    view: humanQueueView,
    route: '/lanes'
  }}
  {template}
/>
<Story
  name="Completed worktrees"
  args={{
    view: completedWorktreesView,
    route: '/lanes'
  }}
  {template}
/>
<Story
  name="Issue detail"
  args={{
    view: activeLanesView,
    route: '/lanes/409'
  }}
  {template}
/>
<Story
  name="Empty lanes"
  args={{
    view: emptyLanesView,
    route: '/lanes'
  }}
  {template}
/>

<style>
  .lanes-page-story-shell {
    width: 100%;
    max-width: 1920px;
    min-height: 1080px;
    padding: var(--space-6);
    color: var(--fg);
    background: var(--bg);
  }

  @media (max-width: 720px) {
    .lanes-page-story-shell {
      width: 100%;
      min-height: 100vh;
      padding: var(--space-4);
    }
  }
</style>
