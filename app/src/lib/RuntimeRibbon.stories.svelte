<script module>
  import { defineMeta } from '@storybook/addon-svelte-csf';
  import RuntimeRibbon from './RuntimeRibbon.svelte';

  const liveSource = {
    label: 'Live',
    trust: 'CLI readback',
    freshness: 'current',
    detail: 'all primary reads completed'
  };

  const pendingSource = {
    label: 'Live',
    trust: 'CLI readback',
    freshness: 'partial',
    detail: 'pending slow reads: sessions, review'
  };

  const { Story } = defineMeta({
    title: 'Components/RuntimeRibbon',
    component: RuntimeRibbon,
    tags: ['autodocs'],
    args: {
      source: liveSource,
      generatedAtLabel: 'checked just now',
      healthy: true,
      fixture: false,
      attentionCount: 2,
      diagnosticCount: 1,
      blockedCount: 0
    }
  });
</script>

<Story name="Live healthy" />
<Story name="Pending reads" args={{ source: pendingSource, attentionCount: 0, diagnosticCount: 2, blockedCount: 1 }} />
<Story name="Fixture mode" args={{ source: liveSource, healthy: false, fixture: true, attentionCount: 3, diagnosticCount: 0, blockedCount: 2 }} />
<Story name="Offline fallback" args={{ source: null, healthy: false, fixture: false, generatedAtLabel: 'not checked', attentionCount: 0, diagnosticCount: 4, blockedCount: 0 }} />
