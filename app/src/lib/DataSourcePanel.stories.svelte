<script module>
  import { defineMeta } from '@storybook/addon-svelte-csf';
  import DataSourcePanel from './DataSourcePanel.svelte';

  const liveSource = {
    label: 'Live local readback',
    freshness: 'checked 2 minutes ago',
    trust: 'CLI and app-server agree',
    detail: 'status, doctor, review, skills, and sessions reads completed',
    tone: 'success'
  };

  const fallbackSource = {
    label: 'Fixture fallback',
    freshness: 'generated for visual QA',
    trust: 'Not operational evidence',
    detail: 'Used when Tauri bridge commands are unavailable in the browser',
    tone: 'warn'
  };

  const { Story } = defineMeta({
    title: 'Operator Components/DataSourcePanel',
    component: DataSourcePanel,
    tags: ['autodocs'],
    args: {
      source: liveSource,
      compact: false
    }
  });
</script>

<Story name="Live source" args={{ source: liveSource }} />
<Story name="Compact" args={{ source: liveSource, compact: true }} />
<Story name="Fixture fallback" args={{ source: fallbackSource }} />
