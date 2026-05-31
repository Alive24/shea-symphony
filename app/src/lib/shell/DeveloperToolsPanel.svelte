<script lang="ts">
  import type { CliLogEntry } from '../uiState.ts';

  type DataMode = 'live' | 'fixture';

  export let width = 340;
  export let dataMode: DataMode = 'live';
  export let refreshRunning = false;
  export let latestLog: CliLogEntry | undefined;
  export let autoloopControl = {
    tauriAvailable: false,
    busy: false,
    running: false,
    mode: 'dry-run',
    workflowPath: 'workflows/shea-symphony.md',
    latestLine: 'No recent autoloop result'
  };
  export let onResizeStart: (event: PointerEvent) => void = () => {};
  export let onHide: () => void = () => {};
  export let onToggleDataMode: () => void = () => {};
  export let onResetFixture: () => void = () => {};
  export let onOpenLogs: () => void = () => {};
  export let onStartDryRun: () => void = () => {};
</script>

<aside
  class="developer-tools-panel"
  aria-label="Developer Tools"
  style={`width: ${width}px`}
>
  <button
    class="developer-tools-resize"
    type="button"
    aria-label="Resize Developer Tools"
    onpointerdown={onResizeStart}
  ></button>
  <header>
    <div>
      <p class="eyebrow">Debug</p>
      <h2>Developer Tools</h2>
    </div>
    <button class="btn btn-ghost" type="button" onclick={onHide}>Hide</button>
  </header>

  <div class="developer-tool-group">
    <span class="developer-tool-label">Data source</span>
    <div class="developer-tool-row">
      <button
        class="mode-switch"
        type="button"
        aria-label="Toggle Live and Fixture data"
        aria-pressed={dataMode === 'fixture'}
        onclick={onToggleDataMode}
      >
        <span>{dataMode === 'fixture' ? 'Fixture' : 'Live'}</span>
      </button>
      {#if dataMode === 'fixture'}
        <button class="mode-reset" type="button" onclick={onResetFixture}>Reset</button>
      {/if}
    </div>
  </div>

  <div class="developer-tool-group">
    <span class="developer-tool-label">CLI</span>
    <button
      class="cli-log-toggle"
      type="button"
      aria-label="Open CLI command log"
      aria-pressed="false"
      onclick={onOpenLogs}
    >
      <span>CLI Logs</span>
      {#if refreshRunning}
        <small>...</small>
      {:else if latestLog}
        <small>{latestLog.status}</small>
      {/if}
    </button>
  </div>

  <div class="developer-tool-group">
    <span class="developer-tool-label">Autoloop</span>
    <button
      class="btn btn-primary developer-tool-action"
      type="button"
      disabled={!autoloopControl.tauriAvailable || autoloopControl.busy || autoloopControl.running}
      onclick={onStartDryRun}
    >
      Start dry-run
    </button>
    <p class="developer-tool-note">
      {autoloopControl.tauriAvailable
        ? `${autoloopControl.mode} · ${autoloopControl.workflowPath}`
        : 'Open in Shea Symphony App desktop shell for live loop control.'}
    </p>
    <p class="developer-tool-note">{autoloopControl.latestLine}</p>
  </div>
</aside>
