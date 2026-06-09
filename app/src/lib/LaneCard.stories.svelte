<script module>
  import { defineMeta } from '@storybook/addon-svelte-csf';
  import LaneCard from './LaneCard.svelte';

  const readyLane = {
    name: 'Main',
    posture: 'ready',
    sourceTone: 'success',
    sourceLabel: 'Live readback',
    href: '#main',
    active: 2,
    retrying: 0,
    blocked: 1,
    latest: 'Issue #442 is pickable after stale claim recovery.'
  };

  const blockedLane = {
    name: 'Review',
    posture: 'blocked',
    sourceTone: 'warn',
    sourceLabel: 'Needs operator decision',
    href: '#review',
    active: 0,
    retrying: 1,
    blocked: 3,
    latest: 'Fresh evidence exists, but Human Review approval has not been recorded.'
  };

  const { Story } = defineMeta({
    title: 'Operator Components/LaneCard',
    component: LaneCard,
    tags: ['autodocs'],
    args: {
      lane: readyLane
    }
  });
</script>

<Story name="Ready lane" args={{ lane: readyLane }} />
<Story name="Blocked lane" args={{ lane: blockedLane }} />
