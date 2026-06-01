<script lang="ts">
  import { JsonTreeView } from '@ark-ui/svelte/json-tree-view';

  export let value: unknown = '';
  export let fallbackLabel = 'plain text';

  $: parsed = parseJson(value);

  function parseJson(input: unknown) {
    if (input == null) return { ok: false, text: '' };
    if (typeof input === 'object') return { ok: true, data: input };
    const text = String(input).trim();
    if (!text) return { ok: false, text: '' };
    try {
      return { ok: true, data: JSON.parse(text) };
    } catch {
      return { ok: false, text };
    }
  }
</script>

{#if parsed.ok}
  <div class="json-log-view">
    <JsonTreeView.Root
      data={parsed.data}
      defaultExpandedDepth={1}
      collapseStringsAfterLength={160}
      maxPreviewItems={6}
      quotesOnKeys={false}
    >
      <JsonTreeView.Tree indentGuide>
        {#snippet arrow()}
          <span class="json-tree-arrow" aria-hidden="true">›</span>
        {/snippet}
      </JsonTreeView.Tree>
    </JsonTreeView.Root>
  </div>
{:else}
  <pre class="json-log-plain" aria-label={fallbackLabel}>{parsed.text}</pre>
{/if}
