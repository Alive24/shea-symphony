<script lang="ts">
  import { cliLogStore } from '../uiState.ts';

  export let onClose: () => void = () => {};

  function formatLogTime(value: string) {
    return new Date(value).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
  }

  function humanDetail(entry) {
    const detail = String(entry.detail || entry.status || '');
    const trimmed = detail.trim();
    if (!trimmed) return `${entry.surface} ${entry.phase}.`;
    if ((trimmed.startsWith('{') && trimmed.endsWith('}')) || (trimmed.startsWith('[') && trimmed.endsWith(']'))) {
      return `${entry.surface} returned structured output.`;
    }
    return trimmed.length > 220 ? `${trimmed.slice(0, 220)}...` : trimmed;
  }
</script>

<div class="modal-backdrop">
  <button class="modal-scrim" type="button" aria-label="Close CLI command log" onclick={onClose}></button>
  <div class="cli-log-modal" role="dialog" aria-modal="true" aria-labelledby="cli-log-title">
    <header>
      <div>
        <p class="eyebrow">Developer Tools</p>
        <h2 id="cli-log-title">CLI Logs</h2>
      </div>
      <button class="btn btn-ghost" type="button" onclick={onClose}>Close</button>
    </header>

    {#if $cliLogStore.length}
      <div class="cli-log-list">
        {#each $cliLogStore as entry}
          <article class="cli-log-row {entry.status}">
            <div>
              <span>{formatLogTime(entry.at)}</span>
              <strong>{entry.surface}</strong>
              <em>{entry.phase}</em>
            </div>
            <p>{humanDetail(entry)}</p>
            <footer>
              <span>{entry.status}</span>
              {#if entry.durationMs != null}
                <span>{Math.round(entry.durationMs)}ms</span>
              {/if}
              {#if entry.args?.length}
                <span>{entry.args[0]} {entry.args[1] ?? ''}</span>
              {/if}
            </footer>
          </article>
        {/each}
      </div>
    {:else}
      <div class="cli-log-empty">
        <strong>No CLI command activity yet</strong>
        <p>Read surfaces will appear here when they start.</p>
      </div>
    {/if}
  </div>
</div>
