<script lang="ts">
  import type { CliLogEntry } from '../uiState.ts';

  type DataMode = 'live' | 'fixture';
  type AutoloopLaneTarget = 'main' | 'review' | 'merge' | 'autoloop';

  const runTargets: { id: AutoloopLaneTarget; label: string }[] = [
    { id: 'main', label: 'Main' },
    { id: 'review', label: 'Review' },
    { id: 'merge', label: 'Merge' },
    { id: 'autoloop', label: 'Autoloop' }
  ];
  const countOptions = [1, 2, 3, 5, 10];

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
  let countMenuOpen: AutoloopLaneTarget | '' = '';
  let countByTarget: Record<AutoloopLaneTarget, number> = {
    main: 1,
    review: 1,
    merge: 1,
    autoloop: 1
  };
  export let onResizeStart: (event: PointerEvent) => void = () => {};
  export let onHide: () => void = () => {};
  export let onToggleDataMode: () => void = () => {};
  export let onResetFixture: () => void = () => {};
  export let onOpenLogs: () => void = () => {};
  export let onStartDryRun: () => void = () => {};
  export let onStartDryRunWithMaxIterations: (maxIterations: number, lane: AutoloopLaneTarget) => void = () => {};
  export let onStartDryRunForLane: (lane: AutoloopLaneTarget) => void = () => {};

  function selectCount(target: AutoloopLaneTarget, value: number) {
    countByTarget = { ...countByTarget, [target]: value };
    countMenuOpen = '';
  }

  function startCountedDryRun(target: AutoloopLaneTarget) {
    onStartDryRunWithMaxIterations(countByTarget[target] ?? 1, target);
  }
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
    <button class="btn btn-ghost" type="button" onclick={onHide}>Collapse</button>
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
    <div class="dev-run-list">
      {#each runTargets as target}
        <div class="dev-run-row">
          <span class="dev-run-name">{target.label}</span>
          <button
            class="btn btn-primary dev-run-button"
            type="button"
            disabled={!autoloopControl.tauriAvailable || autoloopControl.busy || autoloopControl.running}
            onclick={() => target.id === 'autoloop' ? onStartDryRun() : onStartDryRunForLane(target.id)}
          >
            Dry run
          </button>
          <div class="count-picker">
            <button
              class="count-picker-button"
              type="button"
              aria-label={`${target.label} max iterations`}
              aria-haspopup="listbox"
              aria-expanded={countMenuOpen === target.id}
              onclick={() => (countMenuOpen = countMenuOpen === target.id ? '' : target.id)}
            >
              <strong>{countByTarget[target.id]}</strong>
              <span class="select-caret" aria-hidden="true"></span>
            </button>
            {#if countMenuOpen === target.id}
              <div class="count-picker-menu" role="listbox" aria-label={`${target.label} counted run iterations`}>
                {#each countOptions as count}
                  <button
                    type="button"
                    role="option"
                    aria-selected={countByTarget[target.id] === count}
                    onclick={() => selectCount(target.id, count)}
                  >
                    {count}
                  </button>
                {/each}
              </div>
            {/if}
          </div>
          <button
            class="btn btn-ghost dev-run-button"
            type="button"
            disabled={!autoloopControl.tauriAvailable || autoloopControl.busy || autoloopControl.running}
            onclick={() => startCountedDryRun(target.id)}
          >
            Counted
          </button>
        </div>
      {/each}
    </div>
    <p class="developer-tool-note">
      {autoloopControl.tauriAvailable
        ? `${autoloopControl.mode} · ${autoloopControl.workflowPath}`
        : 'Open in Shea Symphony App desktop shell for live loop control.'}
    </p>
    <p class="developer-tool-note">{autoloopControl.latestLine}</p>
  </div>
</aside>
