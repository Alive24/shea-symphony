<script module>
  import { defineMeta } from '@storybook/addon-svelte-csf';
  import { fn } from 'storybook/test';
  import AttentionCard from './AttentionCard.svelte';

  const needHumanInputIssue = {
    id: '#421',
    title: 'Agent Review evidence needs Human Review routing',
    lane: 'Human',
    workerStatus: 'parked',
    state: 'Need Human Input',
    category: 'Need Human Input',
    categoryDetail: 'Work cannot continue without human input.',
    categoryTone: 'project-yellow',
    recommended: 'Review evidence exists but no human decision has been recorded.',
    assignees: ['operator']
  };

  const humanReviewIssue = {
    id: '#425',
    title: 'Parent app-server batch awaits UAT approval',
    lane: 'Human',
    workerStatus: 'review passed',
    state: 'Human Review',
    category: 'Human Review',
    categoryDetail: 'Review passed; waiting on human approval.',
    categoryTone: 'project-pink',
    recommended: 'Run freshness, then approve to Merging or return confirmed findings to Rework.',
    assignees: ['operator', 'review-agent']
  };

  const refreshingIssue = {
    ...needHumanInputIssue,
    refreshing: true
  };

  const { Story } = defineMeta({
    title: 'Components/AttentionCard',
    component: AttentionCard,
    tags: ['autodocs'],
    parameters: {
      layout: 'fullscreen'
    },
    args: {
      issue: needHumanInputIssue,
      handoffTargetLabel: 'Codex App',
      copied: false,
      message: '',
      disabled: false,
      onOpen: fn(),
      onCopy: fn()
    }
  });
</script>

{#snippet template(args)}
  <div class="attention-story-shell">
    <main class="attention-story-frame" aria-label="Storybook app shell preview">
      <section class="human-todo-overview" aria-label="Human operator issue queue">
        <div class="human-todo-rail">
          <AttentionCard {...args} />
        </div>
      </section>
    </main>
  </div>
{/snippet}

<Story name="Need human input" args={{ issue: needHumanInputIssue }} {template} />
<Story name="Human Review" args={{ issue: humanReviewIssue }} {template} />
<Story name="Refreshing disabled" args={{ issue: refreshingIssue, disabled: true }} {template} />

<style>
  .attention-story-shell {
    --attention-story-card-width: 369px;
    --attention-story-card-height: 569px;
    min-height: 100vh;
    padding: var(--space-8);
    color: var(--fg);
    background: var(--bg);
  }

  .attention-story-frame {
    width: min(100%, var(--attention-story-card-width));
    height: var(--attention-story-card-height);
    margin: 0 auto;
  }

  .attention-story-frame .human-todo-overview {
    height: 100%;
  }

  .attention-story-frame .human-todo-rail {
    display: grid;
    grid-auto-flow: row;
    grid-auto-columns: unset;
    grid-template-columns: minmax(0, 1fr);
    height: 100%;
    overflow: visible;
    padding-bottom: 0;
  }

  @media (max-width: 720px) {
    .attention-story-shell {
      padding: var(--space-4);
    }
  }
</style>
